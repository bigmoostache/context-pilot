//! Edit-callback firing — matches file edits against active callbacks and
//! fires them (async + blocking), tagging the last Edit/Write result with
//! warnings, summaries, or a blocking wait sentinel.
//!
//! Split from `pipeline.rs` for the 500-line budget. `pipeline.rs` owns the
//! tool-execution loop; this module owns the post-execution callback dispatch.

use std::fmt::Write as _;

use cp_mod_callback::firing as callback_firing;
use cp_mod_callback::trigger as callback_trigger;
use cp_mod_console::tools::CONSOLE_WAIT_BLOCKING_SENTINEL;

use crate::app::App;

/// Append a note to the last Edit/Write tool result (content + display).
fn append_to_last_edit_result(tool_results: &mut [crate::infra::tools::ToolResult], note: &str) {
    for tr in tool_results.iter_mut().rev() {
        if tr.tool_name == "Edit" || tr.tool_name == "Write" {
            tr.content.push_str(note);
            if let Some(disp) = tr.display.as_mut() {
                disp.push_str(note);
            }
            break;
        }
    }
}

/// Match file edits against active callbacks and fire them (async + blocking),
/// tagging the last Edit/Write result with warnings / summaries / a blocking sentinel.
pub(super) fn fire_edit_callbacks(
    app: &mut App,
    tools: &[cp_base::tools::ToolUse],
    tool_results: &mut [crate::infra::tools::ToolResult],
) {
    // Only collect files from SUCCESSFUL Edit/Write tools (skip failed ones).
    let successful_tools: Vec<_> =
        tools.iter().zip(tool_results.iter()).filter(|(_, r)| !r.is_error).map(|(t, _)| t.clone()).collect();
    let changed_files = callback_trigger::collect_changed_files(&successful_tools);
    if changed_files.is_empty() {
        return;
    }
    let _fg_cb = cp_base::flame!("callbacks");
    let (matched, skip_warnings) = callback_trigger::match_callbacks(&app.state, &changed_files);

    // Inject skip_callbacks warnings into tool results so the AI sees them
    if !skip_warnings.is_empty() {
        let warning_note = format!("\n\n[skip_callbacks warnings: {}]", skip_warnings.join("; "));
        append_to_last_edit_result(tool_results, &warning_note);
    }

    if matched.is_empty() {
        return;
    }
    let (blocking_cbs, async_cbs) = callback_trigger::partition_callbacks(matched);

    // Fire non-blocking callbacks immediately (they run async via watchers)
    if !async_cbs.is_empty() {
        let summaries = callback_firing::fire_async_callbacks(&mut app.state, &async_cbs);
        if !summaries.is_empty() {
            append_to_last_edit_result(tool_results, &format!("\nCallbacks:\n{}", summaries.join("\n")));
        }
    }

    if !blocking_cbs.is_empty() {
        fire_blocking_edit_callbacks(app, &blocking_cbs, tool_results);
    }
}

/// Fire blocking callbacks and tag the last Edit/Write result with a wait
/// sentinel so the pipeline defers all results until the watcher completes.
///
/// CONSTRAINT: each `tool_call` must have exactly 1 `tool_result` — no synthetic
/// `tool_use`/`tool_result` pair is created; the sentinel rides the existing result.
fn fire_blocking_edit_callbacks(
    app: &mut App,
    blocking_cbs: &[callback_trigger::MatchedCallback],
    tool_results: &mut [crate::infra::tools::ToolResult],
) {
    let sentinel_id = format!("cb_block_{}", app.state.next_tool_id);
    app.state.next_tool_id = app.state.next_tool_id.saturating_add(1);

    let summaries = callback_firing::fire_blocking_callbacks(&mut app.state, blocking_cbs, &sentinel_id);

    for tr in tool_results.iter_mut().rev() {
        if tr.tool_name == "Edit" || tr.tool_name == "Write" {
            tr.content = format!("{}{}{}", CONSOLE_WAIT_BLOCKING_SENTINEL, sentinel_id, tr.content);
            // Append spawn failure summaries so they're visible (not silently discarded).
            if !summaries.is_empty() {
                let _r = write!(tr.content, "\nCallbacks:\n{}", summaries.join("\n"));
            }
            break;
        }
    }
}
