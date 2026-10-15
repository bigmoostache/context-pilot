use std::process::Command;
use std::time::Instant;

use super::GH_CMD_TIMEOUT_SECS;
use cp_base::config::constants;
use cp_base::modules::{run_with_timeout, truncate_output};
use cp_base::panels::mark_panels_dirty;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::state::watchers::DYN_PANEL_ID_PLACEHOLDER;
use cp_base::state::watchers::carriers::DynPanel;
use cp_base::tools::async_exec::{ToolOutput, spawn_async_tool};
use cp_base::tools::{ToolResult, ToolUse};

use crate::parse::combine_output;
use crate::types::GithubState;

use super::classify::invalidation::affects;
use super::classify::{CommandClass, classify, validate_gh_command};

/// Max lines for inline output (matches console's `easy_bash` threshold).
const INLINE_MAX_LINES: usize = 150;

/// Max bytes for inline output (~2 000 tokens, matches console threshold).
const INLINE_MAX_BYTES: usize = 8_000;

/// Max execution time (ms) for inline treatment — slow commands always get panels.
const INLINE_MAX_DURATION_MS: u128 = 10_000;

/// Execute a raw gh (GitHub CLI) command.
///
/// Short, fast results are returned **inline** (preserving tempo).
/// Long or slow results create a static `github_result` panel.
/// Mutating commands pre-invalidate cached panels before execution.
pub(crate) fn execute_gh_command(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("gh_exec");
    // Check for GitHub token
    let token = match GithubState::get(state).github_token.as_ref() {
        Some(t) => t.clone(),
        None => {
            return ToolResult::new(
                tool.id.clone(),
                "Error: GITHUB_TOKEN not set. Add GITHUB_TOKEN to your .env file or environment.".to_owned(),
                true,
            );
        }
    };

    let Some(command) = tool.input.get("command").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Error: 'command' parameter is required".to_owned(), true);
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

    // Mutations mark affected panels stale now; the guard holds their refresh
    // back until the `gh` process has exited (see `crate::mutation`).
    let guard = (class == CommandClass::Mutating).then(|| {
        for ctx in &mut state.agent_mut().context {
            if ctx.context_type.as_str() == Kind::GITHUB_RESULT
                && ctx.get_meta_str("result_command").is_some_and(|cmd| affects(&args, cmd))
            {
                ctx.cache_deprecated = true;
            }
        }
        // Always invalidate Git status (PRs/merges can affect it)
        mark_panels_dirty(state, Kind::GIT);
        crate::mutation::begin()
    });

    // All commands: run async, decide inline vs panel on completion.
    let run = GhRun { args, token, command: command.to_owned(), cwd: state.workdir() };
    spawn_async_tool(state, tool, GH_CMD_TIMEOUT_SECS.saturating_add(5), move || {
        let output = run_gh(&run);
        drop(guard);
        output
    })
}

/// One `gh` invocation, owned so it can move into the async worker.
struct GhRun {
    /// Validated argv (without the leading `gh`).
    args: Vec<String>,
    /// Token exported as `GITHUB_TOKEN`/`GH_TOKEN` and redacted from output.
    token: String,
    /// Raw command string, used for panel naming and refresh.
    command: String,
    /// Executing agent's workdir.
    cwd: std::path::PathBuf,
}

/// Run `gh` in the agent's workdir with a timeout and shape the result.
fn run_gh(run: &GhRun) -> ToolOutput {
    let start = Instant::now();

    let mut cmd = Command::new("gh");
    let _r = cmd
        .args(&run.args)
        .current_dir(&run.cwd)
        .env("GITHUB_TOKEN", &run.token)
        .env("GH_TOKEN", &run.token)
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1");

    let result = run_with_timeout(cmd, GH_CMD_TIMEOUT_SECS);
    let elapsed_ms = start.elapsed().as_millis();

    match result {
        Ok(output) => format_gh_output(&output, run, elapsed_ms),
        Err(e) => {
            let content = if e.kind() == std::io::ErrorKind::NotFound {
                "gh CLI not found. Install: https://cli.github.com".to_owned()
            } else {
                format!("Error running gh: {e}")
            };
            ToolOutput::error(content)
        }
    }
}

/// Decide inline-vs-panel for a completed gh command.
fn format_gh_output(output: &std::process::Output, run: &GhRun, elapsed_ms: u128) -> ToolOutput {
    let command = run.command.as_str();
    let token = run.token.as_str();
    let cwd = run.cwd.as_path();
    let is_error = !output.status.success();
    let combined = combine_output(output, token);

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
    let display_content = truncate_output(&combined, constants::MAX_RESULT_CONTENT_BYTES);
    let display_name = if command.len() > 40 {
        format!("{}...", command.get(..command.floor_char_boundary(37)).unwrap_or(""))
    } else {
        command.to_owned()
    };
    let dyn_panel = DynPanel::new(Kind::GITHUB_RESULT.to_owned(), display_name)
        .metadata(vec![
            ("result_command".to_owned(), command.to_owned()),
            ("result_cwd".to_owned(), cwd.to_string_lossy().into_owned()),
        ])
        .content(display_content);
    ToolOutput::new(format!("Panel created: {DYN_PANEL_ID_PLACEHOLDER}"), is_error, None, false).with_panel(dyn_panel)
}
