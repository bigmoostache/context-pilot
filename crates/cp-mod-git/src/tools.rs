use std::process::Command;
use std::time::Instant;

use super::GIT_CMD_TIMEOUT_SECS;
use cp_base::config::constants;
use cp_base::modules::{run_with_timeout, truncate_output};
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::state::watchers::DYN_PANEL_ID_PLACEHOLDER;
use cp_base::state::watchers::carriers::DynPanel;
use cp_base::tools::async_exec::{ToolOutput, spawn_async_tool};
use cp_base::tools::{ToolResult, ToolUse};

use super::classify::{CommandClass, classify_git, validate_git_command};

/// Max lines for inline output (matches console's `easy_bash` threshold).
const INLINE_MAX_LINES: usize = 150;

/// Max bytes for inline output (~2 000 tokens, matches console threshold).
const INLINE_MAX_BYTES: usize = 8_000;

/// Max execution time (ms) for inline treatment — slow commands always get panels.
const INLINE_MAX_DURATION_MS: u128 = 10_000;

/// Execute a raw git command.
///
/// Short, fast results are returned **inline** (preserving tempo).
/// Long or slow results create a static `git_result` panel.
/// Mutating commands pre-invalidate cached panels before execution.
pub(crate) fn execute_git_command(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("git_exec");
    let Some(command) = tool.input.get("command").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Error: 'command' parameter is required".to_owned(), true);
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

    // Pre-invalidate cached panels for mutating commands (needs &mut State).
    if class == CommandClass::Mutating {
        let invalidations = super::cache_invalidation::find_invalidations(command);
        if invalidations.is_empty() {
            cp_base::panels::mark_panels_dirty(state, Kind::GIT_RESULT);
        } else {
            for ctx in &mut state.context {
                if ctx.context_type.as_str() == Kind::GIT_RESULT
                    && let Some(cached_cmd) = ctx.get_meta_str("result_command")
                    && invalidations.iter().any(|re| re.is_match(cached_cmd))
                {
                    ctx.cache_deprecated = true;
                }
            }
        }
    }

    // All commands: run async, decide inline vs panel on completion.
    let command_owned = command.to_owned();
    let github_token = cp_vault::vault().get("github").map(|s| s.expose().to_owned());

    spawn_async_tool(state, tool, GIT_CMD_TIMEOUT_SECS.saturating_add(5), move || {
        let start = Instant::now();

        let mut cmd = Command::new("git");
        let _c = cmd.args(&args).env("GIT_TERMINAL_PROMPT", "0");

        // HTTPS auth via GIT_ASKPASS when GITHUB_TOKEN is available.
        let askpass_tempfile = github_token.as_ref().and_then(|token| {
            let askpass_path = std::env::temp_dir().join(format!("cpilot_askpass_{}", std::process::id()));
            let script = format!("#!/bin/sh\necho '{}'", token.replace('\'', "'\\''"));
            std::fs::write(&askpass_path, &script).is_ok().then(|| {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt as _;
                    drop(std::fs::set_permissions(&askpass_path, std::fs::Permissions::from_mode(0o700)));
                }
                let _ca = cmd.env("GIT_ASKPASS", &askpass_path);
                askpass_path
            })
        });

        let result = run_with_timeout(cmd, GIT_CMD_TIMEOUT_SECS);
        let elapsed_ms = start.elapsed().as_millis();

        // Clean up temp askpass script
        if let Some(path) = &(askpass_tempfile) {
            drop(std::fs::remove_file(path));
        }

        match result {
            Ok(output) => format_git_output(&output, &command_owned, elapsed_ms),
            Err(e) => {
                let content = if e.kind() == std::io::ErrorKind::NotFound {
                    "git not found. Ensure git is installed and on PATH.".to_owned()
                } else {
                    format!("Error running git: {e}")
                };
                ToolOutput::error(content)
            }
        }
    })
}

/// Decide inline-vs-panel for a completed git command.
fn format_git_output(output: &std::process::Output, command: &str, elapsed_ms: u128) -> ToolOutput {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = if stderr.trim().is_empty() {
        stdout.trim().to_owned()
    } else if stdout.trim().is_empty() {
        stderr.trim().to_owned()
    } else {
        format!("{}\n{}", stdout.trim(), stderr.trim())
    };
    let is_error = !output.status.success();

    // Empty output — always inline.
    if combined.is_empty() {
        let content = if is_error {
            "Command failed with no output".to_owned()
        } else {
            "Command completed successfully".to_owned()
        };
        return ToolOutput::new(content, is_error, None, !is_error);
    }

    // Short + fast → inline, preserve tempo.
    let line_count = combined.lines().count();
    if line_count <= INLINE_MAX_LINES && combined.len() <= INLINE_MAX_BYTES && elapsed_ms <= INLINE_MAX_DURATION_MS {
        return ToolOutput::new(combined, is_error, None, !is_error);
    }

    // Long or slow → static panel.
    let combined = truncate_output(&combined, constants::MAX_RESULT_CONTENT_BYTES);
    let display_name = if command.len() > 40 {
        format!("{}...", command.get(..command.floor_char_boundary(37)).unwrap_or(""))
    } else {
        command.to_owned()
    };
    ToolOutput::new(
        format!("Panel created: {DYN_PANEL_ID_PLACEHOLDER}"),
        is_error,
        Some(
            DynPanel::new(Kind::GIT_RESULT.to_owned(), display_name)
                .metadata(vec![("result_command".to_owned(), command.to_owned())])
                .content(combined),
        ),
        false,
    )
}
