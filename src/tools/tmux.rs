use std::process::Command;

use super::{ToolResult, ToolUse};
use crate::constants::{TMUX_BG_SESSION, TMUX_SEND_DELAY_MS, SLEEP_DURATION_SECS};
use crate::state::{estimate_tokens, ContextElement, ContextType, State};

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

    let command = tool.input.get("command")
        .and_then(|v| v.as_str());

    // Ensure background session exists
    if let Err(e) = ensure_bg_session() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: e,
            is_error: true,
        };
    }

    // Create a new window in the background session
    let output = Command::new("tmux")
        .args(["new-window", "-t", TMUX_BG_SESSION, "-d", "-P", "-F", "#{pane_id}"])
        .output();

    let pane_id = match output {
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
    };

    // Run initial command if provided
    if let Some(cmd) = command {
        let _ = Command::new("tmux")
            .args(["send-keys", "-t", &pane_id, cmd, "Enter"])
            .output();
    }

    // Generate context ID
    let context_id = format!("P{}", state.next_context_id);
    state.next_context_id += 1;

    // Add to context
    let name = format!("tmux:{}", pane_id);
    state.context.push(ContextElement {
        id: context_id.clone(),
        context_type: ContextType::Tmux,
        name,
        token_count: 0,
        file_path: None,
        file_hash: None,
        glob_pattern: None,
        glob_path: None,
        tmux_pane_id: Some(pane_id.clone()),
        tmux_lines: Some(lines),
        tmux_last_keys: command.map(|s| s.to_string()),
        tmux_description: Some(description.clone()),
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

    let enter = tool.input.get("enter")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    // Send keys to the pane
    let mut args = vec!["send-keys".to_string(), "-t".to_string(), pane_id.clone(), keys.to_string()];
    if enter {
        args.push("Enter".to_string());
    }

    let output = Command::new("tmux")
        .args(&args)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            // Wait 0.5 seconds for output to appear
            std::thread::sleep(std::time::Duration::from_millis(TMUX_SEND_DELAY_MS));

            // Update last keys in context
            if let Some(ctx) = state.context.iter_mut()
                .find(|c| c.tmux_pane_id.as_deref() == Some(pane_id.as_str()))
            {
                ctx.tmux_last_keys = Some(keys.to_string());
            }

            ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Sent keys to pane {}: {}", pane_id, keys),
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

/// Execute sleep tool (fixed duration from constants)
pub fn execute_sleep(tool: &ToolUse) -> ToolResult {
    std::thread::sleep(std::time::Duration::from_secs(SLEEP_DURATION_SECS));

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Slept for {} second(s)", SLEEP_DURATION_SECS),
        is_error: false,
    }
}

/// Capture content from a tmux pane
pub fn capture_pane_content(pane_id: &str, lines: usize) -> String {
    let output = Command::new("tmux")
        .args(["capture-pane", "-t", pane_id, "-p", "-S", &format!("-{}", lines)])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).to_string()
        }
        _ => String::from("(failed to capture pane content)"),
    }
}

/// Get tmux pane content for all tmux context elements (for API context)
pub fn get_tmux_context(state: &State) -> Vec<(String, String)> {
    state
        .context
        .iter()
        .filter(|c| c.context_type == ContextType::Tmux)
        .filter_map(|c| {
            let pane_id = c.tmux_pane_id.as_ref()?;
            let lines = c.tmux_lines.unwrap_or(50);
            let content = capture_pane_content(pane_id, lines);

            let mut header = format!("tmux pane {}", pane_id);
            if let Some(desc) = &c.tmux_description {
                header = format!("{} ({})", header, desc);
            }
            if let Some(last_keys) = &c.tmux_last_keys {
                header = format!("{} [last: {}]", header, last_keys);
            }

            Some((header, content))
        })
        .collect()
}

/// Refresh token counts for all tmux context elements
pub fn refresh_tmux_context(state: &mut State) {
    for ctx in &mut state.context {
        if ctx.context_type != ContextType::Tmux {
            continue;
        }

        if let Some(pane_id) = &ctx.tmux_pane_id {
            let lines = ctx.tmux_lines.unwrap_or(50);
            let content = capture_pane_content(pane_id, lines);
            ctx.token_count = estimate_tokens(&content);
        }
    }
}
