use std::process::Command;
use std::time::Instant;

use super::GH_CMD_TIMEOUT_SECS;
use cp_base::config::constants;
use cp_base::modules::{run_with_timeout, truncate_output};
use cp_base::panels::mark_panels_dirty;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::state::watchers::{DYN_PANEL_ID_PLACEHOLDER, DynPanel};
use cp_base::tools::async_exec::{ToolOutput, spawn_async_tool};
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::GithubState;

use super::classify::{CommandClass, classify, validate_gh_command};

/// Max lines for inline output (matches console's `easy_bash` threshold).
const INLINE_MAX_LINES: usize = 150;

/// Max bytes for inline output (~2 000 tokens, matches console threshold).
const INLINE_MAX_BYTES: usize = 8_000;

/// Max execution time (ms) for inline treatment — slow commands always get panels.
const INLINE_MAX_DURATION_MS: u128 = 10_000;

/// Redact a GitHub token from command output if accidentally leaked.
fn redact_token(output: &str, token: &str) -> String {
    if token.len() >= 8 && output.contains(token) { output.replace(token, "[REDACTED]") } else { output.to_string() }
}

/// Execute a raw gh (GitHub CLI) command.
///
/// Short, fast results are returned **inline** (preserving tempo).
/// Long or slow results create a static `github_result` panel.
/// Mutating commands pre-invalidate cached panels before execution.
pub(crate) fn execute_gh_command(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("gh_exec");
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

    // Pre-invalidate cached panels for mutating commands (needs &mut State).
    if class == CommandClass::Mutating {
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
    }

    // All commands: run async, decide inline vs panel on completion.
    let command_owned = command.to_string();

    spawn_async_tool(state, tool, GH_CMD_TIMEOUT_SECS.saturating_add(5), move || {
        let start = Instant::now();

        let mut cmd = Command::new("gh");
        let _r = cmd
            .args(&args)
            .env("GITHUB_TOKEN", &token)
            .env("GH_TOKEN", &token)
            .env("GH_PROMPT_DISABLED", "1")
            .env("NO_COLOR", "1");

        let result = run_with_timeout(cmd, GH_CMD_TIMEOUT_SECS);
        let elapsed_ms = start.elapsed().as_millis();

        match result {
            Ok(output) => format_gh_output(&output, &command_owned, &token, elapsed_ms),
            Err(e) => {
                let content = if e.kind() == std::io::ErrorKind::NotFound {
                    "gh CLI not found. Install: https://cli.github.com".to_string()
                } else {
                    format!("Error running gh: {e}")
                };
                ToolOutput { content, is_error: true, create_panel: None, preserves_tempo: false }
            }
        }
    })
}

/// Decide inline-vs-panel for a completed gh command.
fn format_gh_output(output: &std::process::Output, command: &str, token: &str, elapsed_ms: u128) -> ToolOutput {
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
    let combined = redact_token(&combined, token);

    // Empty output — always inline.
    if combined.is_empty() {
        let content = if is_error {
            "Command failed with no output".to_string()
        } else {
            "Command completed successfully".to_string()
        };
        return ToolOutput { content, is_error, create_panel: None, preserves_tempo: !is_error };
    }

    // Short + fast → inline, preserve tempo.
    let line_count = combined.lines().count();
    if line_count <= INLINE_MAX_LINES && combined.len() <= INLINE_MAX_BYTES && elapsed_ms <= INLINE_MAX_DURATION_MS {
        return ToolOutput { content: combined, is_error, create_panel: None, preserves_tempo: !is_error };
    }

    // Long or slow → static panel.
    let combined = truncate_output(&combined, constants::MAX_RESULT_CONTENT_BYTES);
    let display_name = if command.len() > 40 {
        format!("{}...", command.get(..command.floor_char_boundary(37)).unwrap_or(""))
    } else {
        command.to_string()
    };
    ToolOutput {
        content: format!("Panel created: {DYN_PANEL_ID_PLACEHOLDER}"),
        is_error,
        create_panel: Some(DynPanel {
            context_type: Kind::GITHUB_RESULT.to_string(),
            display_name,
            metadata: vec![("result_command".to_string(), command.to_string())],
            content: Some(combined),
        }),
        preserves_tempo: false,
    }
}
