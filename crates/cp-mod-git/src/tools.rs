use std::process::Command;

use super::GIT_CMD_TIMEOUT_SECS;
use cp_base::config::constants::MAX_RESULT_CONTENT_BYTES;
use cp_base::modules::{run_with_timeout, truncate_output};
use cp_base::state::{ContextType, State};
use cp_base::tools::{ToolResult, ToolUse};

use super::classify::{CommandClass, classify_git, validate_git_command};

/// Execute a raw git command.
/// Read-only commands create/reuse `GitResult` panels.
/// Mutating commands execute and return output directly.
pub(crate) fn execute_git_command(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(command) = tool.input.get("command").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Error: 'command' parameter is required".to_string(), true);
    };

    // Validate
    let args = match validate_git_command(command) {
        Ok(a) => a,
        Err(e) => {
            return ToolResult::new(tool.id.clone(), format!("Validation error: {e}"), true);
        }
    };

    // Classify
    let class = classify_git(&args);

    match class {
        CommandClass::ReadOnly => {
            // Search for existing GitResult panel with same command
            let existing_idx = state.context.iter().position(|c| {
                c.context_type == ContextType::GIT_RESULT && c.get_meta_str("result_command") == Some(command)
            });

            if let Some(idx) = existing_idx {
                // Reuse existing panel — mark deprecated to trigger re-fetch
                state.context[idx].cache_deprecated = true;
                let panel_id = state.context[idx].id.clone();
                ToolResult::new(tool.id.clone(), format!("Panel updated: {panel_id}"), false)
            } else {
                // Create new GitResult panel
                let panel_id = state.next_available_context_id();
                let uid = format!("UID_{}_P", state.global_next_uid);
                state.global_next_uid += 1;

                let mut elem = cp_base::state::make_default_context_element(
                    &panel_id,
                    ContextType::new(ContextType::GIT_RESULT),
                    command,
                    true,
                );
                elem.uid = Some(uid);
                elem.set_meta("result_command", &command.to_string());
                state.context.push(elem);

                ToolResult::new(tool.id.clone(), format!("Panel created: {panel_id}"), false)
            }
        }
        CommandClass::Mutating => {
            // Execute directly with timeout
            let mut cmd = Command::new("git");
            let _ = cmd.args(&args).env("GIT_TERMINAL_PROMPT", "0");

            // If GITHUB_TOKEN is available, create a temporary askpass script
            // so git push/pull/fetch can authenticate via HTTPS automatically.
            let github_token = std::env::var("GITHUB_TOKEN").ok();
            let _askpass_tempfile = if let Some(ref token) = github_token {
                let mut tmp = std::env::temp_dir();
                tmp.push(format!("cpilot_askpass_{}", std::process::id()));
                let script = format!("#!/bin/sh\necho '{}'", token.replace('\'', "'\\''"));
                if std::fs::write(&tmp, &script).is_ok() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o700)).ok();
                    }
                    let _ = cmd.env("GIT_ASKPASS", &tmp);
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
                let _ = std::fs::remove_file(path).ok();
            }

            // Heuristic-based cache invalidation for GitResult panels
            let invalidations = super::cache_invalidation::find_invalidations(command);
            if invalidations.is_empty() {
                // Unknown mutating command -> blanket invalidation (safe default)
                cp_base::panels::mark_panels_dirty(state, ContextType::new(ContextType::GIT_RESULT));
            } else {
                for ctx in &mut state.context {
                    if ctx.context_type == ContextType::GIT_RESULT
                        && let Some(cmd) = ctx.get_meta_str("result_command")
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
                    ToolResult::new(
                        tool.id.clone(),
                        if combined.is_empty() {
                            if is_error {
                                "Command failed with no output".to_string()
                            } else {
                                "Command completed successfully".to_string()
                            }
                        } else {
                            combined
                        },
                        is_error,
                    )
                }
                Err(e) => {
                    let content = if e.kind() == std::io::ErrorKind::NotFound {
                        "git not found. Ensure git is installed and on PATH.".to_string()
                    } else {
                        format!("Error running git: {e}")
                    };
                    ToolResult::new(tool.id.clone(), content, true)
                }
            }
        }
    }
}
