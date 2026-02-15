use std::process::Command;

use crate::constants::{GIT_CMD_TIMEOUT_SECS, MAX_RESULT_CONTENT_BYTES};
use crate::modules::{run_with_timeout, truncate_output};
use crate::state::{ContextType, State};
use crate::tools::{ToolResult, ToolUse};

use super::classify::{CommandClass, classify_git, validate_git_command};

/// Execute a raw git command.
/// Read-only commands create/reuse GitResult panels.
/// Mutating commands execute and return output directly.
pub fn execute_git_command(tool: &ToolUse, state: &mut State) -> ToolResult {
    let command = match tool.input.get("command").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Error: 'command' parameter is required".to_string(),
                is_error: true,
            };
        }
    };

    // Validate
    let args = match validate_git_command(command) {
        Ok(a) => a,
        Err(e) => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Validation error: {}", e),
                is_error: true,
            };
        }
    };

    // Classify
    let class = classify_git(&args);

    match class {
        CommandClass::ReadOnly => {
            // Search for existing GitResult panel with same command
            let existing_idx = state
                .context
                .iter()
                .position(|c| c.context_type == ContextType::GitResult && c.result_command.as_deref() == Some(command));

            if let Some(idx) = existing_idx {
                // Reuse existing panel — mark deprecated to trigger re-fetch
                state.context[idx].cache_deprecated = true;
                let panel_id = state.context[idx].id.clone();
                ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Panel updated: {}", panel_id),
                    is_error: false,
                }
            } else {
                // Create new GitResult panel
                let panel_id = state.next_available_context_id();
                let uid = format!("UID_{}_P", state.global_next_uid);
                state.global_next_uid += 1;

                let mut elem =
                    crate::modules::make_default_context_element(&panel_id, ContextType::GitResult, command, true);
                elem.uid = Some(uid);
                elem.result_command = Some(command.to_string());
                state.context.push(elem);

                ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Panel created: {}", panel_id),
                    is_error: false,
                }
            }
        }
        CommandClass::Mutating => {
            // Execute directly with timeout
            let mut cmd = Command::new("git");
            cmd.args(&args).env("GIT_TERMINAL_PROMPT", "0");

            // If GITHUB_TOKEN is available, create a temporary askpass script
            // so git push/pull/fetch can authenticate via HTTPS automatically.
            let _askpass_tempfile = if let Some(ref token) = state.github_token {
                let mut tmp = std::env::temp_dir();
                tmp.push(format!("cpilot_askpass_{}", std::process::id()));
                let script = format!("#!/bin/sh\necho '{}'", token.replace('\'', "'\\''"));
                if std::fs::write(&tmp, &script).is_ok() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o700));
                    }
                    cmd.env("GIT_ASKPASS", &tmp);
                    Some(tmp) // kept alive until end of scope, then cleaned up
                } else {
                    None
                }
            } else {
                None
            };

            let result = run_with_timeout(cmd, GIT_CMD_TIMEOUT_SECS);

            // Clean up temp askpass script
            if let Some(ref path) = _askpass_tempfile {
                let _ = std::fs::remove_file(path);
            }

            // Heuristic-based cache invalidation for GitResult panels
            let invalidations = super::cache_invalidation::find_invalidations(command);
            if invalidations.is_empty() {
                // Unknown mutating command → blanket invalidation (safe default)
                crate::core::panels::mark_panels_dirty(state, ContextType::GitResult);
            } else {
                for ctx in &mut state.context {
                    if ctx.context_type == ContextType::GitResult
                        && let Some(ref cmd) = ctx.result_command
                        && invalidations.iter().any(|re| re.is_match(cmd))
                    {
                        ctx.cache_deprecated = true;
                    }
                }
            }
            // P6 (Git) always invalidated via .git/ file watcher — no action needed here

            match result {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let combined = if stderr.trim().is_empty() {
                        stdout.trim().to_string()
                    } else if stdout.trim().is_empty() {
                        stderr.trim().to_string()
                    } else {
                        format!("{}\n{}", stdout.trim(), stderr.trim())
                    };
                    let is_error = !output.status.success();
                    let combined = truncate_output(&combined, MAX_RESULT_CONTENT_BYTES);
                    ToolResult {
                        tool_use_id: tool.id.clone(),
                        content: if combined.is_empty() {
                            if is_error {
                                "Command failed with no output".to_string()
                            } else {
                                "Command completed successfully".to_string()
                            }
                        } else {
                            combined
                        },
                        is_error,
                    }
                }
                Err(e) => {
                    let content = if e.kind() == std::io::ErrorKind::NotFound {
                        "git not found. Ensure git is installed and on PATH.".to_string()
                    } else {
                        format!("Error running git: {}", e)
                    };
                    ToolResult { tool_use_id: tool.id.clone(), content, is_error: true }
                }
            }
        }
    }
}

/// Configure the P6 git status panel.
/// Supports: show_diffs, show_logs, log_args, diff_base.
pub fn execute_configure_p6(tool: &ToolUse, state: &mut State) -> ToolResult {
    let show_diffs = tool.input.get("show_diffs").and_then(|v| v.as_bool());
    let show_logs = tool.input.get("show_logs").and_then(|v| v.as_bool());
    let log_args = tool.input.get("log_args").and_then(|v| v.as_str());
    let diff_base = tool.input.get("diff_base").and_then(|v| v.as_str());

    let mut changes = Vec::new();

    // Update show_diffs
    if let Some(v) = show_diffs {
        state.git_show_diffs = v;
        changes.push(format!("show_diffs={}", v));
    }

    // Update show_logs
    if let Some(v) = show_logs {
        state.git_show_logs = v;
        if v {
            // Fetch git log content
            let args_str = log_args.or(state.git_log_args.as_deref()).unwrap_or("-10 --oneline");
            let args_vec: Vec<&str> = args_str.split_whitespace().collect();

            let mut cmd = Command::new("git");
            cmd.arg("log");
            for arg in args_vec {
                cmd.arg(arg);
            }
            match cmd.output() {
                Ok(output) if output.status.success() => {
                    state.git_log_content = Some(String::from_utf8_lossy(&output.stdout).to_string());
                }
                _ => {
                    state.git_log_content = Some("Failed to fetch git log".to_string());
                }
            }
        } else {
            state.git_log_content = None;
        }
        changes.push(format!("show_logs={}", v));
    }

    // Update log_args (even without toggling show_logs)
    if let Some(args) = log_args {
        state.git_log_args = Some(args.to_string());
        // Re-fetch if logs are currently shown
        if state.git_show_logs {
            let args_vec: Vec<&str> = args.split_whitespace().collect();
            let mut cmd = Command::new("git");
            cmd.arg("log");
            for arg in args_vec {
                cmd.arg(arg);
            }
            match cmd.output() {
                Ok(output) if output.status.success() => {
                    state.git_log_content = Some(String::from_utf8_lossy(&output.stdout).to_string());
                }
                _ => {
                    state.git_log_content = Some("Failed to fetch git log".to_string());
                }
            }
        }
        changes.push(format!("log_args={}", args));
    }

    // Update diff_base
    if let Some(base) = diff_base {
        if base.is_empty() || base == "none" || base == "null" {
            // Clear diff base — revert to default
            state.git_diff_base = None;
            changes.push("diff_base=<cleared>".to_string());
        } else {
            // Validate the ref
            let check = Command::new("git").args(["rev-parse", "--verify", base]).output();
            match check {
                Ok(output) if output.status.success() => {
                    state.git_diff_base = Some(base.to_string());
                    changes.push(format!("diff_base={}", base));
                }
                _ => {
                    return ToolResult {
                        tool_use_id: tool.id.clone(),
                        content: format!("Error: '{}' is not a valid git ref", base),
                        is_error: true,
                    };
                }
            }
        }
    }

    if changes.is_empty() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "No changes specified. Use show_diffs, show_logs, log_args, or diff_base parameters.".to_string(),
            is_error: true,
        };
    }

    // Mark P6 as deprecated to refresh
    crate::core::panels::mark_panels_dirty(state, ContextType::Git);

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("P6 configured: {}", changes.join(", ")),
        is_error: false,
    }
}
