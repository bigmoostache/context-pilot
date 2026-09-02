//! Post-tool-execution checks: panel readiness and deferred sleeps.
//!
//! Extracted from `tool_pipeline.rs` to keep that module under the 500-line limit.
//! Both functions are non-blocking polls called from the main event loop.

use std::sync::mpsc::Sender;

use std::fmt::Write as _;

use crate::app::panels::now_ms;
use crate::app::run::streaming::has_dirty_panels;
use crate::infra::api::StreamEvent;

use crate::app::App;

/// Non-blocking check: if we're waiting for file panels to load,
/// check if they're ready (or timed out) and continue streaming.
pub(crate) fn check_waiting_for_panels(app: &mut App, tx: &Sender<StreamEvent>) {
    if !app.state.flags.lifecycle.waiting_for_panels {
        return;
    }

    let panels_ready = !has_dirty_panels(&app.state);
    let timed_out = now_ms().saturating_sub(app.wait_started_ms) >= 5_000;

    if panels_ready || timed_out {
        app.state.flags.lifecycle.waiting_for_panels = false;
        app.state.flags.ui.dirty = true;
        crate::app::run::streaming::continue_streaming(app, tx);
    }
}

/// Non-blocking check: if a tool requested a sleep (e.g., `console_sleep`),
/// wait for the timer to expire, then deprecate tmux panels and continue
/// through the normal `wait_for_panels` → `continue_streaming` pipeline.
pub(crate) fn check_deferred_sleep(app: &mut App, tx: &Sender<StreamEvent>) {
    if !app.deferred_tool_sleeping {
        return;
    }

    if now_ms() < app.deferred_tool_sleep_until_ms {
        return; // Still sleeping — keep processing input normally
    }

    app.deferred_tool_sleeping = false;
    app.deferred_tool_sleep_until_ms = 0;
    app.state.flags.ui.dirty = true;

    // Deferred sleep expired — continue streaming
    crate::app::run::streaming::continue_streaming(app, tx);
}

// ─── Todo focus-scoping + work-hygiene nudge (thread-owned tasks rework) ──────
//
// Two post-tool-batch steps, hosted in the main crate because they bridge
// `cp-mod-threads` (`FocusState`) and `cp-mod-todo` (`TodoState`), which never
// import each other (design §8 dependency-injection rule).

/// Push the focused thread id onto `TodoState` so the Todo panel scopes to it.
///
/// When focus changed, the panel's previous content is stale (it pointed at the
/// old thread's tasks), so the Todo panel is forced fresh immediately — mirrors
/// the threads panel's force-refresh: deprecate the cache and set
/// `freeze_count = u8::MAX` (the sanctioned "not frozen" sentinel that forces
/// the Fresh branch), and break tempo (design §4.1 focus-change row).
pub(crate) fn sync_todo_focus(app: &mut App) {
    let focused = cp_mod_threads::types::FocusState::get(&app.state).focused_thread_id.clone();
    if cp_mod_todo::tools::set_focus_filter(&mut app.state, focused) {
        for ctx in &mut app.state.context {
            if ctx.context_type.as_str() == crate::state::Kind::TODO {
                ctx.cache_deprecated = true;
                ctx.freeze_count = u8::MAX;
                break;
            }
        }
        app.state.tempo = false;
    }
}

/// Push the focused thread id onto `ScratchpadState` so the Scratchpad panel
/// scopes to it (thread-owned scratchpad rework — the twin of
/// [`sync_todo_focus`]).
///
/// When focus changed, the panel's previous content is stale (it pointed at the
/// old thread's cells), so the Scratchpad panel is forced fresh immediately:
/// deprecate the cache and set `freeze_count = u8::MAX` (the sanctioned "not
/// frozen" sentinel that forces the Fresh branch), and break tempo.
pub(crate) fn sync_scratchpad_focus(app: &mut App) {
    let focused = cp_mod_threads::types::FocusState::get(&app.state).focused_thread_id.clone();
    if cp_mod_scratchpad::tools::set_focus_filter(&mut app.state, focused) {
        for ctx in &mut app.state.context {
            if ctx.context_type.as_str() == crate::state::Kind::SCRATCHPAD {
                ctx.cache_deprecated = true;
                ctx.freeze_count = u8::MAX;
                break;
            }
        }
        app.state.tempo = false;
    }
}

/// Fire a **once-per-focused-thread** work-hygiene nudge (FR11) when the focused
/// thread has (a) no tasks, or (b) planned work but nothing in progress.
///
/// The nudge is injected into the chat but not accumulated (mirrors the Think
/// reminder): a `Custom` spine notification created then immediately marked
/// processed. `TodoState.nudged_thread` tracks the last-nudged thread so it does
/// not re-fire on every tool call; it resets when the condition clears (a task
/// is created / a WIP item is picked) or when focus moves to another thread.
pub(crate) fn maybe_hygiene_nudge(app: &mut App) {
    let Some(tid) = cp_mod_threads::types::FocusState::get(&app.state).focused_thread_id.clone() else {
        return; // No focused thread → no nudge.
    };
    let (no_tasks, has_planned, has_in_progress) = {
        use cp_mod_todo::types::{TodoState, TodoStatus};
        let ts = TodoState::get(&app.state);
        let items: Vec<&cp_mod_todo::types::TodoItem> =
            ts.todos.iter().filter(|t| t.thread_id == tid && t.status != TodoStatus::Cancelled).collect();
        let no_tasks = items.is_empty();
        let has_planned = items.iter().any(|t| t.status == TodoStatus::Planned);
        let has_in_progress = items.iter().any(|t| t.status == TodoStatus::InProgress);
        (no_tasks, has_planned, has_in_progress)
    };
    let should_nudge = no_tasks || (has_planned && !has_in_progress);
    let already = cp_mod_todo::types::TodoState::get(&app.state).nudged_thread.as_deref() == Some(tid.as_str());

    if !should_nudge {
        // Condition cleared for this thread — reset its fire-once flag.
        if already {
            cp_mod_todo::types::TodoState::get_mut(&mut app.state).nudged_thread = None;
        }
        return;
    }
    if already {
        return; // Already nudged this thread (fire-once).
    }

    let msg = if no_tasks {
        "This thread has no tasks yet \u{2014} use Think's todo to sketch a checklist before you dig in."
    } else {
        "You have planned tasks but none in progress \u{2014} mark one in_progress so your plan is visible."
    };
    let id = cp_mod_spine::types::SpineState::create_notification(
        &mut app.state,
        cp_mod_spine::types::NotificationType::Custom,
        "todo_hygiene".to_owned(),
        msg.to_owned(),
    );
    let _found = cp_mod_spine::types::SpineState::mark_notification_processed(&mut app.state, &id);
    cp_mod_todo::types::TodoState::get_mut(&mut app.state).nudged_thread = Some(tid);
}

/// Auto-promote a declared task to `in_progress` after an opted-in tool runs,
/// and append the synthetic task tree to that tool's result when the flip
/// actually happened (T686).
///
/// When an executed tool that opted into task declaration (`declares_task`)
/// carries a valid `task_id` belonging to the focused thread and that task is
/// still `planned`, flip it to `in_progress` — so declaring work on a task marks
/// it started, live. Finished/cancelled tasks are left untouched (pre-flight
/// already warned); an already-`in_progress` task is a silent no-op.
///
/// Because the flips are applied **in order**, only the FIRST tool that declares
/// a given planned task causes its change; a later tool declaring the same id
/// sees it already in progress (no change → no append). Per T686, the tree (plus
/// the multi-in-progress-leaf warning) is appended ONLY to the result of a tool
/// whose `task_id` genuinely changed a status — matching the spec's "valid AND
/// implied a status change" trigger.
///
/// The flip mutates `TodoState` only; the `emit_task_lists` main-loop chokepoint
/// observes the change and emits the `TaskListChanged` delta, so the web aside
/// re-renders in real time. Like `todo_mark` (FR7) it does NOT deprecate the
/// Todo panel — a status flip is deliberately cheap.
///
/// `tools` and `tool_results` are order-aligned (same construction as the
/// finalize phase), so `tool_results[i]` is the result for `tools[i]`.
pub(crate) fn promote_declared_tasks(
    app: &mut App,
    tools: &[cp_base::tools::ToolUse],
    tool_results: &mut [crate::infra::tools::ToolResult],
) {
    let Some(focused) = cp_mod_threads::types::FocusState::get(&app.state).focused_thread_id.clone() else {
        return; // No focused thread → task_id not enforced, nothing to promote.
    };

    for (tool, result) in tools.iter().zip(tool_results.iter_mut()) {
        let Some(task_id) = declared_focused_task(app, tool, &focused) else {
            continue;
        };
        if flip_planned_to_in_progress(app, &focused, &task_id) {
            // A real status change → append the thread's task tree (+ warning).
            let annex = cp_mod_todo::tree::result_annex(&app.state, &focused);
            let _w = write!(result.content, "\n\n{annex}");
        }
    }
}

/// The trimmed, non-empty `task_id` an opted-in (`declares_task`) tool declared
/// that resolves to a task of the `focused` thread — else `None`.
fn declared_focused_task(app: &App, tool: &cp_base::tools::ToolUse, focused: &str) -> Option<String> {
    let opted_in = app.state.tools.iter().any(|d| d.id == tool.name && d.declares_task);
    if !opted_in {
        return None;
    }
    let task_id =
        tool.input.get("task_id").and_then(serde_json::Value::as_str).map(str::trim).filter(|s| !s.is_empty())?;
    let owned =
        cp_mod_todo::types::TodoState::get(&app.state).todos.iter().any(|t| t.id == task_id && t.thread_id == focused);
    owned.then(|| task_id.to_owned())
}

/// Flip `task_id` from `Planned` to `InProgress` (owned by `focused`). Returns
/// whether a change actually occurred (i.e. the task was `Planned`).
fn flip_planned_to_in_progress(app: &mut App, focused: &str, task_id: &str) -> bool {
    let ts = cp_mod_todo::types::TodoState::get_mut(&mut app.state);
    if let Some(item) = ts.todos.iter_mut().find(|t| t.id == task_id && t.thread_id == focused)
        && item.status == cp_mod_todo::types::TodoStatus::Planned
    {
        item.status = cp_mod_todo::types::TodoStatus::InProgress;
        return true;
    }
    false
}
