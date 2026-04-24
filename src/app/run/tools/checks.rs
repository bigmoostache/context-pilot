//! Post-tool-execution checks: panel readiness, deferred sleeps, and question forms.
//!
//! Extracted from `tool_pipeline.rs` to keep that module under the 500-line limit.
//! All three functions are non-blocking polls called from the main event loop.

use std::sync::mpsc::Sender;

use crate::app::panels::now_ms;
use crate::infra::api::StreamEvent;
use crate::state::{Message, MsgKind, MsgStatus, ToolResultRecord};

use super::pipeline::accumulate_pending_token_stats;
use crate::app::run::streaming::{has_dirty_file_panels, has_dirty_panels, trigger_dirty_panel_refresh};

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

/// Non-blocking check: if the user has resolved a pending question form,
/// replace the `__QUESTION_PENDING__` placeholder with the real answer and
/// resume the tool pipeline (create result message + continue streaming).
pub(crate) fn check_question_form(app: &mut App, tx: &Sender<StreamEvent>) {
    // Only check if we have pending tool results waiting on a question
    if app.pending_question_tool_results.is_none() {
        return;
    }

    // Check if form is resolved
    let resolved = app.state.get_ext::<cp_base::ui::question_form::PendingForm>().is_some_and(|f| f.resolved);

    if !resolved {
        return;
    }

    // Extract the resolved form and remove it from state
    let Some(form) = app
        .state
        .module_data
        .remove(&std::any::TypeId::of::<cp_base::ui::question_form::PendingForm>())
        .and_then(|v| v.downcast::<cp_base::ui::question_form::PendingForm>().ok())
    else {
        return;
    };

    let result_json =
        form.result_json.unwrap_or_else(|| r#"{"dismissed":true,"message":"User declined to answer"}"#.to_string());

    // Replace placeholder in pending tool results
    let Some(mut tool_results) = app.pending_question_tool_results.take() else {
        return;
    };
    for tr in &mut tool_results {
        if tr.content == "__QUESTION_PENDING__" {
            tr.content.clone_from(&result_json);
        }
    }

    // Now resume the normal pipeline: create result message and continue streaming
    let result_id = format!("R{}", app.state.next_result_id);
    let result_global_uid = format!("UID_{}_R", app.state.global_next_uid);
    app.state.next_result_id = app.state.next_result_id.saturating_add(1);
    app.state.global_next_uid = app.state.global_next_uid.saturating_add(1);
    let tool_result_records: Vec<ToolResultRecord> = tool_results
        .iter()
        .map(|r| ToolResultRecord {
            tool_use_id: r.tool_use_id.clone(),
            content: r.content.clone(),
            display: r.display.clone(),
            is_error: r.is_error,
            tool_name: r.tool_name.clone(),
        })
        .collect();
    let result_msg = Message {
        id: result_id,
        uid: Some(result_global_uid),
        role: "user".to_string(),
        msg_type: MsgKind::ToolResult,
        content: String::new(),
        content_token_count: 0,
        status: MsgStatus::Full,
        tool_uses: Vec::new(),
        tool_results: tool_result_records,
        input_tokens: 0,
        timestamp_ms: now_ms(),
    };
    app.save_message_async(&result_msg);
    app.state.messages.push(result_msg);

    // Check if reload was requested
    if app.state.flags.lifecycle.reload_pending {
        return;
    }

    // Create new assistant message for continued streaming
    let assistant_id = format!("A{}", app.state.next_assistant_id);
    let assistant_global_uid = format!("UID_{}_A", app.state.global_next_uid);
    app.state.next_assistant_id = app.state.next_assistant_id.saturating_add(1);
    app.state.global_next_uid = app.state.global_next_uid.saturating_add(1);
    let new_assistant_msg = Message {
        id: assistant_id,
        uid: Some(assistant_global_uid),
        role: "assistant".to_string(),
        msg_type: MsgKind::TextMessage,
        content: String::new(),
        content_token_count: 0,
        status: MsgStatus::Full,
        tool_uses: Vec::new(),
        tool_results: Vec::new(),
        input_tokens: 0,
        timestamp_ms: now_ms(),
    };
    app.state.messages.push(new_assistant_msg);

    app.state.streaming_estimated_tokens = 0;

    // Accumulate token stats from intermediate stream
    accumulate_pending_token_stats(app);

    app.save_state_async();
    app.state.flags.ui.dirty = true;

    // Continue streaming
    let _ = trigger_dirty_panel_refresh(&app.state, &app.cache_tx);
    if has_dirty_file_panels(&app.state) {
        app.state.flags.lifecycle.waiting_for_panels = true;
        app.wait_started_ms = now_ms();
    } else {
        crate::app::run::streaming::continue_streaming(app, tx);
    }
}
