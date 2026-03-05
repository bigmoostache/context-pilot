use std::process::Command;

use super::GH_CMD_TIMEOUT_SECS;
use super::classify::CommandClass;
use cp_base::config::constants;
use cp_base::modules::{run_with_timeout, truncate_output};
use cp_base::panels::mark_panels_dirty;
use cp_base::state::context::{Kind, make_default_entry};
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::GithubState;

use super::classify::{classify, validate_gh_command};

/// Redact a GitHub token from command output if accidentally leaked.
fn redact_token(output: &str, token: &str) -> String {
    if token.len() >= 8 && output.contains(token) { output.replace(token, "[REDACTED]") } else { output.to_string() }
}

/// Execute a raw gh (GitHub CLI) command.
/// Read-only commands create/reuse `GithubResult` panels.
/// Mutating commands execute and return output directly.
pub(crate) fn execute_gh_command(tool: &ToolUse, state: &mut State) -> ToolResult {
    // Check for GitHub token
    let token = match &GithubState::get(state).github_token {
        Some(t) => t.clone(),
        None => {
            return ToolResult::new(
                tool.id.clone(),
                "Error: GITHUB_TOKEN not set. Add GITHUB_TOKEN to your .env file or environment.".to_string(),
                true,
            );
        }
    };

    let Some(command) = tool.input.get("command").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Error: 'command' parameter is required".to_string(), true);
    };

    // Validate
    let args = match validate_gh_command(command) {
        Ok(a) => a,
        Err(e) => {
            return ToolResult::new(tool.id.clone(), format!("Validation error: {e}"), true);
        }
    };

    // Classify
    let class = classify(&args);

    match class {
        CommandClass::ReadOnly => {
            // Search for existing GithubResult panel with same command
            let existing_idx = state.context.iter().position(|c| {
                c.context_type.as_str() == Kind::GITHUB_RESULT && c.get_meta_str("result_command") == Some(command)
            });

            if let Some(ctx) = existing_idx.and_then(|idx| state.context.get_mut(idx)) {
                // Reuse existing panel — mark deprecated to trigger re-fetch
                ctx.cache_deprecated = true;
                let panel_id = ctx.id.clone();
                ToolResult::new(tool.id.clone(), format!("Panel updated: {panel_id}"), false)
            } else {
                // Create new GithubResult panel
                let panel_id = state.next_available_context_id();
                let uid = format!("UID_{}_P", state.global_next_uid);
                state.global_next_uid = state.global_next_uid.saturating_add(1);

                let mut elem = make_default_entry(&panel_id, Kind::new(Kind::GITHUB_RESULT), command, true);
                elem.uid = Some(uid);
                elem.set_meta("result_command", &command.to_string());
                state.context.push(elem);

                ToolResult::new(tool.id.clone(), format!("Panel created: {panel_id}"), false)
            }
        }
        CommandClass::Mutating => {
            // Execute directly with timeout
            let mut cmd = Command::new("gh");
            let _r = cmd
                .args(&args)
                .env("GITHUB_TOKEN", &token)
                .env("GH_TOKEN", &token)
                .env("GH_PROMPT_DISABLED", "1")
                .env("NO_COLOR", "1");
            let result = run_with_timeout(cmd, GH_CMD_TIMEOUT_SECS);

            // Invalidate affected panels using heuristics
            let invalidations = super::cache_invalidation::find_invalidations(command);
            for ctx in &mut state.context {
                if ctx.context_type.as_str() == Kind::GITHUB_RESULT {
                    let matches = ctx
                        .get_meta_str("result_command")
                        .is_some_and(|cached_cmd| invalidations.iter().any(|re| re.is_match(cached_cmd)));
                    if matches {
                        ctx.cache_deprecated = true;
                    }
                }
            }
            // Always invalidate Git status (PRs/merges can affect it)
            mark_panels_dirty(state, Kind::GIT);

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
                    let combined = truncate_output(&combined, constants::MAX_RESULT_CONTENT_BYTES);
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
                        "gh CLI not found. Install: https://cli.github.com".to_string()
                    } else {
                        format!("Error running gh: {e}")
                    };
                    ToolResult::new(tool.id.clone(), content, true)
                }
            }
        }
    }
}
