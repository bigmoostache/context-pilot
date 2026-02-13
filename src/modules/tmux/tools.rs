use std::process::Command;

use crate::tools::{ToolResult, ToolUse};
use crate::constants::{TMUX_BG_SESSION, TMUX_SEND_DELAY_MS, SLEEP_DURATION_SECS};
use crate::state::{ContextElement, ContextType, State};

/// Ensure the background tmux session exists
fn ensure_bg_session() -> Result<(), String> {
    // Check if session exists
    let check = Command::new("tmux")
        .args(["has-session", "-t", TMUX_BG_SESSION])
        .output();

    match check {
        Ok(out) if out.status.success() => Ok(()), // Session exists
        _ => {
            // Create detached session
            let create = Command::new("tmux")
                .args(["new-session", "-d", "-s", TMUX_BG_SESSION])
                .output();

            match create {
                Ok(out) if out.status.success() => Ok(()),
                Ok(out) => Err(format!("Failed to create tmux session: {}", String::from_utf8_lossy(&out.stderr))),
                Err(e) => Err(format!("Failed to run tmux: {}", e)),
            }
        }
    }
}

/// Execute create_tmux_pane tool
pub fn execute_create_pane(tool: &ToolUse, state: &mut State) -> ToolResult {
    let description = tool.input.get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("Terminal")
        .to_string();

    let lines = tool.input.get("lines")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;

    let requested_pane_id = tool.input.get("pane_id")
        .and_then(|v| v.as_str());

    // Determine the pane ID: use existing pane if provided, otherwise create a new one
    let pane_id = if let Some(pid) = requested_pane_id {
        // Verify the pane exists
        let check = Command::new("tmux")
            .args(["display-message", "-t", pid, "-p", "#{pane_id}"])
            .output();
        match check {
            Ok(out) if out.status.success() => {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            }
            _ => {
                return ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Pane '{}' not found. Check tmux pane IDs.", pid),
                    is_error: true,
                };
            }
        }
    } else {
        // No pane_id provided — create a new one in the background session
        if let Err(e) = ensure_bg_session() {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: e,
                is_error: true,
            };
        }

        let output = Command::new("tmux")
            .args(["new-window", "-t", TMUX_BG_SESSION, "-d", "-P", "-F", "#{pane_id}"])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            }
            Ok(out) => {
                return ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Failed to create tmux pane: {}", String::from_utf8_lossy(&out.stderr)),
                    is_error: true,
                };
            }
            Err(e) => {
                return ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Failed to run tmux command: {}", e),
                    is_error: true,
                };
            }
        }
    };

    // Check if this pane is already being monitored
    if state.context.iter().any(|c| c.tmux_pane_id.as_deref() == Some(&pane_id)) {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Pane {} is already being monitored", pane_id),
            is_error: true,
        };
    }

    // Generate context ID (fills gaps) and UID
    let context_id = state.next_available_context_id();
    let uid = format!("UID_{}_P", state.global_next_uid);
    state.global_next_uid += 1;

    // Add to context (cache will be populated by background system)
    let name = format!("tmux:{}", pane_id);
    state.context.push(ContextElement {
        id: context_id.clone(),
        uid: Some(uid),
        context_type: ContextType::Tmux,
        name,
        token_count: 0,
        file_path: None,
        file_hash: None,
        glob_pattern: None,
        glob_path: None,
        grep_pattern: None,
        grep_path: None,
        grep_file_pattern: None,
        tmux_pane_id: Some(pane_id.clone()),
        tmux_lines: Some(lines),
        tmux_last_keys: None,
        tmux_description: Some(description.clone()),
        result_command: None,
        result_command_hash: None,
        skill_prompt_id: None,
        cached_content: None,
        history_messages: None,
        cache_deprecated: true, // Mark as deprecated so background refresh runs
        cache_in_flight: false,
        last_refresh_ms: crate::core::panels::now_ms(),
        last_polled_ms: 0,
        content_hash: None,
        tmux_last_lines_hash: None,
        current_page: 0,
        total_pages: 1,
        full_token_count: 0,
        panel_cache_hit: false,
        panel_total_cost: 0.0,
    });

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Created tmux {} pane {} ({})", context_id, pane_id, description),
        is_error: false,
    }
}

/// Execute edit_tmux_config tool
pub fn execute_edit_config(tool: &ToolUse, state: &mut State) -> ToolResult {
    // Accept either pane_id or context_id
    let identifier = tool.input.get("pane_id")
        .or_else(|| tool.input.get("context_id"))
        .and_then(|v| v.as_str());

    let identifier = match identifier {
        Some(id) => id,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'pane_id' or 'context_id' parameter".to_string(),
                is_error: true,
            };
        }
    };

    // Find the context by context_id or pane_id
    let ctx = state.context.iter_mut()
        .find(|c| c.id == identifier || c.tmux_pane_id.as_deref() == Some(identifier));

    match ctx {
        Some(c) => {
            let mut changes = Vec::new();

            if let Some(desc) = tool.input.get("description").and_then(|v| v.as_str()) {
                c.tmux_description = Some(desc.to_string());
                changes.push(format!("description='{}'", desc));
            }

            if let Some(lines) = tool.input.get("lines").and_then(|v| v.as_u64()) {
                c.tmux_lines = Some(lines as usize);
                changes.push(format!("lines={}", lines));
            }

            if changes.is_empty() {
                ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: "No changes specified".to_string(),
                    is_error: true,
                }
            } else {
                let pane_id = c.tmux_pane_id.as_deref().unwrap_or(identifier);
                ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Updated pane {}: {}", pane_id, changes.join(", ")),
                    is_error: false,
                }
            }
        }
        None => {
            ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Pane '{}' not found. Use pane_id (e.g. %23) or context_id (e.g. P7)", identifier),
                is_error: true,
            }
        }
    }
}

/// Resolve a pane identifier to the actual tmux pane ID
/// Accepts either a context_id (like "P7") or a raw pane_id (like "%23")
fn resolve_pane_id(identifier: &str, state: &State) -> Option<String> {
    // First, try to find by context_id
    if let Some(ctx) = state.context.iter().find(|c| c.id == identifier) {
        return ctx.tmux_pane_id.clone();
    }
    // Then try to find by pane_id directly
    if let Some(ctx) = state.context.iter().find(|c| c.tmux_pane_id.as_deref() == Some(identifier)) {
        return ctx.tmux_pane_id.clone();
    }
    // If it looks like a tmux pane ID (starts with %), return it as-is
    if identifier.starts_with('%') {
        return Some(identifier.to_string());
    }
    None
}

/// Execute tmux_send_keys tool
pub fn execute_send_keys(tool: &ToolUse, state: &mut State) -> ToolResult {
    // Accept either pane_id or context_id
    let identifier = tool.input.get("pane_id")
        .or_else(|| tool.input.get("context_id"))
        .and_then(|v| v.as_str());

    let identifier = match identifier {
        Some(id) => id,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'pane_id' or 'context_id' parameter".to_string(),
                is_error: true,
            };
        }
    };

    let pane_id = match resolve_pane_id(identifier, state) {
        Some(id) => id,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Pane '{}' not found. Use pane_id (e.g. %23) or context_id (e.g. P7)", identifier),
                is_error: true,
            };
        }
    };

    let keys = match tool.input.get("keys").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'keys' parameter".to_string(),
                is_error: true,
            };
        }
    };

    // Reject bare "Enter" since it's sent automatically
    if keys.eq_ignore_ascii_case("enter") {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "console_send_keys already sends Enter automatically after your keys — no need to send it separately.".to_string(),
            is_error: true,
        };
    }

    // Reject git/gh commands — use the dedicated git_execute and gh_execute tools instead.
    // Check all segments of compound commands (split on &&, ||, ;, |) to catch
    // patterns like "cd /foo && git push" or "echo done; gh pr list".
    let has_git_gh = keys.split(|c: char| c == '&' || c == '|' || c == ';')
        .map(|segment| segment.trim())
        .filter(|s| !s.is_empty())
        .any(|segment| {
            segment.starts_with("git ") || segment == "git"
                || segment.starts_with("gh ") || segment == "gh"
        });
    if has_git_gh {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "Use the git_execute or gh_execute tools instead of running git/gh commands through console_send_keys.".to_string(),
            is_error: true,
        };
    }

    // Send keys to the pane (always followed by Enter)
    let args = vec!["send-keys".to_string(), "-t".to_string(), pane_id.clone(), keys.to_string(), "Enter".to_string()];

    let output = Command::new("tmux")
        .args(&args)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            // Set a timer for deferred tmux capture (non-blocking).
            // The main loop will wait for this timer before continuing the stream,
            // ensuring the LLM sees fresh tmux panel content.
            // Use max() so multiple send_keys in one batch don't shorten each other's wait.
            let new_deadline = crate::core::panels::now_ms() + TMUX_SEND_DELAY_MS;
            state.tool_sleep_until_ms = state.tool_sleep_until_ms.max(new_deadline);
            state.tool_sleep_needs_tmux_refresh = true;

            // Update last_keys on the context element
            let context_id = if let Some(ctx) = state.context.iter_mut()
                .find(|c| c.tmux_pane_id.as_deref() == Some(pane_id.as_str()))
            {
                ctx.tmux_last_keys = Some(keys.to_string());
                Some(ctx.id.clone())
            } else {
                None
            };

            let panel_msg = context_id
                .map(|id| format!(". Content up to date in panel {}", id))
                .unwrap_or_default();

            ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Sent keys to pane {}: {}{}", pane_id, keys, panel_msg),
                is_error: false,
            }
        }
        Ok(out) => {
            ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Failed to send keys: {}", String::from_utf8_lossy(&out.stderr)),
                is_error: true,
            }
        }
        Err(e) => {
            ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Failed to run tmux command: {}", e),
                is_error: true,
            }
        }
    }
}

/// Execute sleep tool (non-blocking).
/// Sets a timer on state instead of blocking the main thread.
/// The main event loop checks the timer and refreshes tmux panels when it expires.
pub fn execute_sleep(tool: &ToolUse, state: &mut State) -> ToolResult {
    let sleep_ms = SLEEP_DURATION_SECS * 1000;
    let new_deadline = crate::core::panels::now_ms() + sleep_ms;
    state.tool_sleep_until_ms = state.tool_sleep_until_ms.max(new_deadline);

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Slept for {} second(s)", SLEEP_DURATION_SECS),
        is_error: false,
    }
}
