//! Reverie event processing — polls reverie streams and dispatches tools.
//! Supports multiple concurrent reveries (one per agent type).

use std::sync::mpsc;

use crate::app::App;
use crate::app::reverie::{streaming, tools};
use crate::infra::api::StreamEvent;
use crate::state::persistence::save_state;
use cp_base::config::REVERIE;
use cp_mod_queue::types::QueueState;

/// Check if any reverie needs a stream started (state has reverie but no stream).
/// Called from the main event loop.
pub(super) fn maybe_start_reverie_stream(app: &mut App) {
    // Collect agent_ids that need a stream started
    let needs_start: Vec<String> = app
        .state
        .reveries
        .iter()
        .filter(|(agent_id, r)| r.is_streaming && !app.reverie_streams.contains_key(*agent_id))
        .map(|(agent_id, _)| agent_id.clone())
        .collect();

    for agent_id in needs_start {
        let (tx, rx) = mpsc::channel();
        streaming::start_reverie_stream(&mut app.state, &agent_id, tx);
        let _r = app
            .reverie_streams
            .insert(agent_id, super::super::ReverieStream { rx, pending_tools: Vec::new(), report_called: false });
    }
}

/// Poll all reverie streams for events and process them.
/// Called from the main event loop, AFTER main stream events.
pub(super) fn process_reverie_events(app: &mut App) {
    // Collect agent_ids that have streams
    let agent_ids: Vec<String> = app.reverie_streams.keys().cloned().collect();

    for agent_id in agent_ids {
        // Drain all events from this reverie's stream
        let events: Vec<StreamEvent> = match app.reverie_streams.get(&agent_id) {
            Some(s) => s.rx.try_iter().collect(),
            None => continue,
        };

        for evt in events {
            app.state.flags.ui.dirty = true;
            if apply_reverie_event(app, &agent_id, evt) {
                break; // This agent's stream is gone, move to next
            }
        }
    }
}

/// Apply one drained reverie stream event to `agent_id`'s state. Returns `true`
/// when the agent's session was destroyed (caller must stop draining it).
fn apply_reverie_event(app: &mut App, agent_id: &str, evt: StreamEvent) -> bool {
    match evt {
        StreamEvent::ToolProgress { .. } => {} // Reveries run in background — no UI preview
        StreamEvent::Chunk(text) => append_reverie_chunk(app, agent_id, &text),
        StreamEvent::ToolUse(tool) => {
            if let Some(stream) = app.reverie_streams.get_mut(agent_id) {
                stream.pending_tools.push(tool);
            }
        }
        StreamEvent::Done { .. } => {
            if let Some(rev) = app.state.reveries.get_mut(agent_id) {
                if let Some(msg) = rev.messages.last_mut() {
                    msg.status = crate::state::MsgStatus::Full;
                }
                rev.is_streaming = false;
            }
        }
        StreamEvent::Error(e) => {
            destroy_reverie_on_error(app, agent_id, &e);
            return true;
        }
    }
    false
}

/// Append a streamed text chunk to the reverie's trailing assistant message,
/// opening a fresh assistant message when none is in progress.
fn append_reverie_chunk(app: &mut App, agent_id: &str, text: &str) {
    let Some(rev) = app.state.reveries.get_mut(agent_id) else { return };
    if rev.messages.last().is_none_or(|m| m.role != "assistant") {
        rev.messages.push(crate::state::Message::new_text(
            format!("rev-{}", rev.messages.len()),
            "assistant",
            String::new(),
        ));
    }
    if let Some(msg) = rev.messages.last_mut() {
        msg.content.push_str(text);
    }
}

/// A reverie stream errored: notify, discard its queued actions, destroy the
/// agent's session (non-critical — reveries are best-effort).
fn destroy_reverie_on_error(app: &mut App, agent_id: &str, err: &str) {
    let _notif = cp_mod_spine::types::SpineState::create_notification(
        &mut app.state,
        cp_mod_spine::types::NotificationType::Custom,
        "Reverie".to_owned(),
        format!("Reverie '{agent_id}' error: {err}. Destroying session."),
    );
    QueueState::get_mut(&mut app.state).clear();
    drop(app.state.reveries.remove(agent_id));
    drop(app.reverie_streams.remove(agent_id));
}

/// Execute pending reverie tool calls for all active reveries.
/// Called from the main event loop, AFTER main tools are processed.
pub(super) fn handle_reverie_tools(app: &mut App) {
    let _fg = cp_base::flame!("reverie_tools");
    // Collect agent_ids that have pending tools
    let agent_ids: Vec<String> =
        app.reverie_streams.iter().filter(|(_, s)| !s.pending_tools.is_empty()).map(|(id, _)| id.clone()).collect();

    for agent_id in agent_ids {
        // Take pending tools from the stream state
        let pending = match app.reverie_streams.get_mut(&agent_id) {
            Some(s) => std::mem::take(&mut s.pending_tools),
            None => continue,
        };

        let mut tool_results = Vec::new();

        for tool in &pending {
            // Increment tool call count
            if let Some(rev) = app.state.reveries.get_mut(&agent_id) {
                rev.tool_call_count = rev.tool_call_count.saturating_add(1);
            }

            if reverie_over_tool_cap(app, &agent_id) {
                break; // Move to next agent
            }

            let Some(result) = dispatch_one_reverie_tool(app, &agent_id, tool) else {
                break; // Report / destroy sentinel — agent gone, move on
            };

            record_reverie_tool_exchange(app, &agent_id, tool, &result);
            tool_results.push(result);
        }

        restream_reverie_if_alive(app, &agent_id, &tool_results);
    }
}

/// Guard rail: if the agent exceeded `REVERIE_TOOL_CAP`, notify, clear its queue,
/// and destroy the session. Returns `true` when force-stopped.
fn reverie_over_tool_cap(app: &mut App, agent_id: &str) -> bool {
    let cap = crate::infra::constants::REVERIE_TOOL_CAP;
    if app.state.reveries.get(agent_id).is_none_or(|r| r.tool_call_count <= cap) {
        return false;
    }
    let _notif_cap = cp_mod_spine::types::SpineState::create_notification(
        &mut app.state,
        cp_mod_spine::types::NotificationType::Custom,
        "Reverie".to_owned(),
        format!("Tool cap ({cap}) reached for '{agent_id}'. Force-stopping."),
    );
    QueueState::get_mut(&mut app.state).clear();
    drop(app.state.reveries.remove(agent_id));
    drop(app.reverie_streams.remove(agent_id));
    true
}

/// Dispatch one reverie tool through the router. `None` means the agent was
/// destroyed (Report sentinel) — the caller must stop processing it.
/// `Queue_execute`/`Queue_pause` and the Report sentinel get special handling;
/// everything else is either queued (reverie queue active) or dispatched.
fn dispatch_one_reverie_tool(
    app: &mut App,
    agent_id: &str,
    tool: &cp_base::tools::ToolUse,
) -> Option<crate::infra::tools::ToolResult> {
    if tool.name == "Queue_execute" {
        // Reverie doesn't need flushed tools (no callbacks) — just the summary
        return Some(super::tools::queue_flush::execute_queue_flush(tool, &mut app.state).0);
    }
    if tool.name == "Queue_pause" {
        if let Some(rev) = app.state.reveries.get_mut(agent_id) {
            rev.queue_active = false;
        }
        return Some(crate::infra::tools::ToolResult::new(tool.id.clone(), "Queue paused (reverie)".into(), false));
    }

    if let Some(result) = tools::dispatch_reverie_tool(tool, &app.state) {
        // Check for Report sentinel
        if result.content.starts_with("REVERIE_REPORT:") {
            destroy_reverie_on_report(app, agent_id, &result.content);
            return None;
        }
        return Some(result);
    }

    // Tool is allowed — check if reverie queue is active
    let should_queue =
        app.state.reveries.get(agent_id).is_some_and(|r| r.queue_active) && !QueueState::is_queue_tool(&tool.name);
    if should_queue {
        let qs = QueueState::get_mut(&mut app.state);
        let idx = qs.enqueue(cp_mod_queue::types::QueuedToolCall::new(
            tool.name.clone(),
            tool.id.clone(),
            tool.input.clone(),
        ));
        return Some(crate::infra::tools::ToolResult::new(tool.id.clone(), format!("Queued as #{idx}"), false));
    }
    // Execute normally through module dispatch
    let active = app.state.active_modules.clone();
    Some(crate::modules::dispatch_tool(tool, &mut app.state, &active))
}

/// Handle a `REVERIE_REPORT:` sentinel: emit the summary notification, mark the
/// stream reported, clear its queued actions, and destroy the session.
fn destroy_reverie_on_report(app: &mut App, agent_id: &str, content: &str) {
    let summary = content.strip_prefix("REVERIE_REPORT:").unwrap_or("Completed");
    let _notif_report = cp_mod_spine::types::SpineState::create_notification(
        &mut app.state,
        cp_mod_spine::types::NotificationType::Custom,
        "Reverie".to_owned(),
        summary.to_owned(),
    );
    if let Some(stream) = app.reverie_streams.get_mut(agent_id) {
        stream.report_called = true;
    }
    // Clear queued actions from this reverie (shared queue) but do NOT touch
    // QueueState.active — that's the main worker's toggle.
    QueueState::get_mut(&mut app.state).clear();
    drop(app.state.reveries.remove(agent_id));
    drop(app.reverie_streams.remove(agent_id));
    save_state(&app.state);
}

/// Record a reverie tool call + its result as a `ToolCall`/`ToolResult` message pair.
fn record_reverie_tool_exchange(
    app: &mut App,
    agent_id: &str,
    tool: &cp_base::tools::ToolUse,
    result: &crate::infra::tools::ToolResult,
) {
    let Some(rev) = app.state.reveries.get_mut(agent_id) else { return };
    rev.messages.push(crate::state::Message::new_tool_call(
        format!("rev-tc-{}", rev.messages.len()),
        None,
        vec![crate::state::ToolUseRecord::new(tool.id.clone(), tool.name.clone(), tool.input.clone())],
    ));
    rev.messages.push(crate::state::Message::new_tool_result(
        format!("rev-tr-{}", rev.messages.len()),
        None,
        vec![
            crate::state::ToolResultRecord::new(result.tool_use_id.clone(), result.content.clone(), result.is_error)
                .display(result.display.clone())
                .tldr(result.tldr.clone())
                .tool_name(result.tool_name.clone()),
        ],
    ));
}

/// After executing a batch of reverie tools, re-stream the agent if it's still
/// alive (trims trailing whitespace, flips `is_streaming`, spawns a fresh stream).
fn restream_reverie_if_alive(app: &mut App, agent_id: &str, tool_results: &[crate::infra::tools::ToolResult]) {
    if tool_results.is_empty() || !app.state.reveries.contains_key(agent_id) {
        return;
    }
    if let Some(rev) = app.state.reveries.get_mut(agent_id) {
        for msg in &mut rev.messages {
            if msg.role == "assistant" {
                msg.content = msg.content.trim_end().to_owned();
            }
        }
        rev.is_streaming = true;
    }
    let (tx, rx) = mpsc::channel();
    streaming::start_reverie_stream(&mut app.state, agent_id, tx);
    let _r = app.reverie_streams.insert(
        agent_id.to_owned(),
        super::super::ReverieStream { rx, pending_tools: Vec::new(), report_called: false },
    );
}

/// Check if any reverie ended without calling Report.
/// If so, inject a user message telling it to call Report, then re-stream.
pub(super) fn check_reverie_end_turn(app: &mut App) {
    // Collect agent_ids of reveries that have stopped streaming
    let stopped: Vec<String> =
        app.state.reveries.iter().filter(|(_, r)| !r.is_streaming).map(|(id, _)| id.clone()).collect();

    for agent_id in stopped {
        let report_called = app.reverie_streams.get(&agent_id).is_some_and(|s| s.report_called);

        if report_called {
            continue; // All good
        }

        // End turn without Report — check retry limit
        let retries = app.state.reveries.get(&agent_id).map_or(0, |r| r.report_retries);
        if retries >= 1 {
            // Max retries reached — force destroy
            let _notif_end = cp_mod_spine::types::SpineState::create_notification(
                &mut app.state,
                cp_mod_spine::types::NotificationType::Custom,
                "Reverie".to_owned(),
                format!("Reverie '{agent_id}' ended without Report after retry. Force-destroying."),
            );
            QueueState::get_mut(&mut app.state).clear();
            drop(app.state.reveries.remove(&agent_id));
            drop(app.reverie_streams.remove(&agent_id));
            continue;
        }

        // Inject a user message telling the LLM to call Report, then re-stream
        if let Some(rev) = app.state.reveries.get_mut(&agent_id) {
            rev.report_retries = rev.report_retries.saturating_add(1);
            rev.is_streaming = true;

            for msg in &mut rev.messages {
                if msg.role == "assistant" {
                    msg.content = msg.content.trim_end().to_owned();
                }
            }

            rev.messages.push(crate::state::Message::new_text(
                format!("rev-nudge-{}", rev.messages.len()),
                "user",
                REVERIE.report_nudge.trim_end().to_owned(),
            ));
        }

        let (tx, rx) = mpsc::channel();
        streaming::start_reverie_stream(&mut app.state, &agent_id, tx);
        let _r = app
            .reverie_streams
            .insert(agent_id, super::super::ReverieStream { rx, pending_tools: Vec::new(), report_called: false });
    }
}
