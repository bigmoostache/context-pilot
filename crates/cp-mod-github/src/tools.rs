use std::process::Command;

use cp_base::classify::CommandClass;
use cp_base::constants::{GH_CMD_TIMEOUT_SECS, MAX_RESULT_CONTENT_BYTES};
use cp_base::modules::{run_with_timeout, truncate_output};
use cp_base::panels::mark_panels_dirty;
use cp_base::state::{ContextType, State, make_default_context_element};
use cp_base::tools::{ToolResult, ToolUse};

use super::classify::{classify_gh, validate_gh_command};

/// Redact a GitHub token from command output if accidentally leaked.
fn redact_token(output: &str, token: &str) -> String {
    if token.len() >= 8 && output.contains(token) { output.replace(token, "[REDACTED]") } else { output.to_string() }
}

/// Execute a raw gh (GitHub CLI) command.
/// Read-only commands create/reuse GithubResult panels.
/// Mutating commands execute and return output directly.
pub fn execute_gh_command(tool: &ToolUse, state: &mut State) -> ToolResult {
    // Check for GitHub token
    let token = match &state.github_token {
        Some(t) => t.clone(),
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Error: GITHUB_TOKEN not set. Add GITHUB_TOKEN to your .env file or environment.".to_string(),
                is_error: true,
            };
        }
    };

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
    let args = match validate_gh_command(command) {
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
    let class = classify_gh(&args);

    match class {
        CommandClass::ReadOnly => {
            // Search for existing GithubResult panel with same command
            let existing_idx = state.context.iter().position(|c| {
                c.context_type == ContextType::GITHUB_RESULT && c.result_command.as_deref() == Some(command)
            });

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
                // Create new GithubResult panel
                let panel_id = state.next_available_context_id();
                let uid = format!("UID_{}_P", state.global_next_uid);
                state.global_next_uid += 1;

                let mut elem = make_default_context_element(&panel_id, ContextType::new(ContextType::GITHUB_RESULT), command, true);
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
            let mut cmd = Command::new("gh");
            cmd.args(&args)
                .env("GITHUB_TOKEN", &token)
                .env("GH_TOKEN", &token)
                .env("GH_PROMPT_DISABLED", "1")
                .env("NO_COLOR", "1");
            let result = run_with_timeout(cmd, GH_CMD_TIMEOUT_SECS);

            // Invalidate affected panels using heuristics
            let invalidations = super::cache_invalidation::find_invalidations(command);
            for ctx in &mut state.context {
                if ctx.context_type == ContextType::GITHUB_RESULT
                    && let Some(ref cmd) = ctx.result_command
                    && invalidations.iter().any(|re| re.is_match(cmd))
                {
                    ctx.cache_deprecated = true;
                }
            }
            // Always invalidate Git status (PRs/merges can affect it)
            mark_panels_dirty(state, ContextType::new(ContextType::GIT));

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
                    let combined = redact_token(&combined, &token);
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
                        "gh CLI not found. Install: https://cli.github.com".to_string()
                    } else {
                        format!("Error running gh: {}", e)
                    };
                    ToolResult { tool_use_id: tool.id.clone(), content, is_error: true }
                }
            }
        }
    }
}
