//! Post-tool-execution checks: panel readiness and deferred sleeps.
//!
//! Extracted from `tool_pipeline.rs` to keep that module under the 500-line limit.
//! Both functions are non-blocking polls called from the main event loop.

use std::sync::mpsc::Sender;

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

/// Auto-promote a declared task to `in_progress` after an opted-in tool runs.
///
/// When an executed tool that opted into task declaration (`declares_task`)
/// carries a valid `task_id` belonging to the focused thread and that task is
/// still `planned`, flip it to `in_progress` — so declaring work on a task marks
/// it started, live. Finished/cancelled tasks are left untouched (pre-flight
/// already warned); an already-`in_progress` task is a silent no-op.
///
/// The flip mutates `TodoState` only; the `emit_task_lists` main-loop chokepoint
/// observes the change and emits the `TaskListChanged` delta, so the web aside
/// re-renders in real time. Like `todo_mark` (FR7) it does NOT deprecate the
/// Todo panel — a status flip is deliberately cheap.
pub(crate) fn promote_declared_tasks(app: &mut App, tools: &[cp_base::tools::ToolUse]) {
    let Some(focused) = cp_mod_threads::types::FocusState::get(&app.state).focused_thread_id.clone() else {
        return; // No focused thread → task_id not enforced, nothing to promote.
    };

    // Collect the distinct, valid, focused-thread task ids declared by opted-in
    // tools in this batch (borrow of app.state dropped before the mutation).
    let mut to_promote: Vec<String> = Vec::new();
    for tool in tools {
        let opted_in = app.state.tools.iter().any(|d| d.id == tool.name && d.declares_task);
        if !opted_in {
            continue;
        }
        let Some(task_id) =
            tool.input.get("task_id").and_then(serde_json::Value::as_str).map(str::trim).filter(|s| !s.is_empty())
        else {
            continue;
        };
        if !to_promote.iter().any(|id| id == task_id) {
            to_promote.push(task_id.to_owned());
        }
    }
    if to_promote.is_empty() {
        return;
    }

    // Flip only planned tasks owned by the focused thread; leave done/cancelled/
    // already-in_progress untouched.
    let ts = cp_mod_todo::types::TodoState::get_mut(&mut app.state);
    for item in &mut ts.todos {
        if item.thread_id == focused
            && item.status == cp_mod_todo::types::TodoStatus::Planned
            && to_promote.iter().any(|id| id == &item.id)
        {
            item.status = cp_mod_todo::types::TodoStatus::InProgress;
        }
    }
}
