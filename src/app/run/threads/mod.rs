//! Thread-related helpers for the main event loop.
//!
//! Extracted from `lifecycle.rs` to keep it under the 500-line limit.
//! Contains auto-`Read` injection and `MY_TURN` thread detection here; the
//! bridge command intake/application + live-vitals emission live in the
//! sibling [`bridge`] submodule (the two halves split a single file that had
//! outgrown the 500-line limit).

mod bridge;
mod messages;
pub(super) use bridge::{bridge_active, emit_vitals, poll_bridge_commands};
pub(super) use messages::emit_messages;

use crate::app::App;
use crate::app::panels::now_ms;
use cp_base::state::data::message::{MsgKind, MsgStatus, ToolResultRecord, ToolUseRecord};
use cp_base::tools::ToolUse;
use cp_mod_spine::types::{NotificationType, SpineState};
use cp_mod_threads::types::{FocusState, ThreadStatus, ThreadsState};

/// Inject a synthetic `Read` tool call when auto-continuation fires
/// for a thread notification while the AI is unfocused.
///
/// The Read is 100% deterministic in this scenario — the AI would
/// always call it anyway. Injecting it saves a full round-trip:
/// the AI starts streaming with focus already set and thread content
/// visible, so it can immediately `Send` its response.
///
/// Thread selection priority:
/// 1. Extract thread IDs from the synthetic message (notification content)
///    and pick the first one that's still `MY_TURN`.
/// 2. Fall back to any `MY_TURN` thread if no notification thread ID matches.
///
/// Modifies the message list by popping the empty streaming-target
/// assistant message, inserting the Read `tool_use` + `tool_result` pair,
/// then pushing a new empty assistant for streaming.
pub(super) fn maybe_inject_auto_read(app: &mut App) {
    // Only inject when unfocused + a MY_TURN thread exists.
    let fs = FocusState::get(&app.state);
    if fs.focused_thread_id.is_some() {
        return;
    }

    // Extract thread IDs from the synthetic message that triggered this
    // continuation. The synthetic is the second-to-last message (before
    // the empty assistant streaming target pushed by `apply_continuation`).
    let candidate_ids: Vec<String> = app
        .state
        .messages
        .len()
        .checked_sub(2)
        .and_then(|idx| app.state.messages.get(idx))
        .filter(|m| m.role == "user" && m.content.starts_with("/* Auto-continuation:"))
        .map(|m| extract_thread_ids(&m.content))
        .unwrap_or_default();

    let ts = ThreadsState::get(&app.state);

    // Prefer the thread the notification is about; fall back to any MY_TURN.
    // Archived threads are LLM-invisible (T9) and never auto-read.
    let my_turn = candidate_ids
        .iter()
        .find_map(|tid| {
            ts.threads.iter().find(|t| t.id == *tid && !t.archived && t.status == ThreadStatus::MyTurn)
        })
        .or_else(|| ts.threads.iter().find(|t| !t.archived && t.status == ThreadStatus::MyTurn));

    let Some(thread) = my_turn else {
        return;
    };
    let tid = thread.id.clone();

    // Pop the empty assistant (streaming target) — we'll push a fresh
    // one after the injected Read messages.
    let Some(streaming_target) = app.state.messages.pop() else {
        return;
    };

    // Build a synthetic ToolUse for Read.
    let tool_use_id = format!("auto_read_{tid}");
    let input = serde_json::json!({
        "thread_id": tid,
        "intent": "Focus on thread",
        "verb": "Reading",
    });

    let tool_use = ToolUse { id: tool_use_id.clone(), name: "Read".into(), input: input.clone() };

    // Execute Read — this sets focus and returns formatted messages.
    let result = cp_mod_threads::tools::execute_read(&tool_use, &mut app.state);

    // Create assistant message carrying the tool_use record.
    let tool_call_msg = crate::state::Message {
        id: format!("T{}", app.state.next_tool_id),
        uid: Some(format!("UID_{}_T", app.state.global_next_uid)),
        role: "assistant".into(),
        content: String::new(),
        msg_type: MsgKind::ToolCall,
        status: MsgStatus::Full,
        tool_uses: vec![ToolUseRecord { id: tool_use_id.clone(), name: "Read".into(), input }],
        tool_results: vec![],
        input_tokens: 0,
        content_token_count: 0,
        timestamp_ms: now_ms(),
    };
    app.state.next_tool_id = app.state.next_tool_id.saturating_add(1);
    app.state.global_next_uid = app.state.global_next_uid.saturating_add(1);

    // Create tool_result message (user role).
    let result_msg = crate::state::Message {
        id: format!("R{}", app.state.next_result_id),
        uid: Some(format!("UID_{}_R", app.state.global_next_uid)),
        role: "user".into(),
        content: String::new(),
        msg_type: MsgKind::ToolResult,
        status: MsgStatus::Full,
        tool_uses: vec![],
        tool_results: vec![ToolResultRecord {
            tool_use_id,
            content: result.content,
            display: None,
            tldr: None,
            is_error: result.is_error,
            tool_name: "Read".into(),
        }],
        input_tokens: 0,
        content_token_count: 0,
        timestamp_ms: now_ms(),
    };
    app.state.next_result_id = app.state.next_result_id.saturating_add(1);
    app.state.global_next_uid = app.state.global_next_uid.saturating_add(1);

    // Persist both injected messages.
    app.save_message_async(&tool_call_msg);
    app.save_message_async(&result_msg);

    // Push: tool_call → tool_result → streaming target.
    app.state.messages.push(tool_call_msg);
    app.state.messages.push(result_msg);
    app.state.messages.push(streaming_target);
}

/// Notify when idle and a thread has `MY_TURN` status.
///
/// Debounced via `FocusState::notified_my_turn_id` — fires once per
/// thread transition to `MY_TURN`, cleared when the AI sends a reply
/// (which sets `THEIR_TURN`).
pub(super) fn check_my_turn_threads(app: &mut App) {
    if app.state.flags.stream.phase.is_streaming() {
        return;
    }

    let threads = ThreadsState::get(&app.state);
    // Archived threads are invisible to the LLM (T9) — they never nudge.
    let my_turn = threads.threads.iter().find(|t| !t.archived && t.status == ThreadStatus::MyTurn);

    let Some(thread) = my_turn else {
        // No MY_TURN threads — clear debounce.
        FocusState::get_mut(&mut app.state).notified_my_turn_id = None;
        return;
    };

    let tid = thread.id.clone();
    let tname = thread.name.clone();

    // Debounce: already notified about this exact thread.
    // Re-fire only when the previous notification was consumed (processed)
    // but the AI still hasn't addressed the thread — creating a persistent
    // nudge loop until the thread is actually handled.
    if FocusState::get(&app.state).notified_my_turn_id.as_deref() == Some(&tid) {
        let has_unprocessed =
            SpineState::get(&app.state).notifications.iter().any(|n| !n.is_processed() && n.source == "my_turn_thread");
        if has_unprocessed {
            return; // Previous nudge still pending — don't spam
        }
        // Previous nudge consumed but thread still MY_TURN — clear debounce to re-fire
    }

    FocusState::get_mut(&mut app.state).notified_my_turn_id = Some(tid.clone());

    let content = format!(
        "Thread \"{tname}\" ({tid}) is MY_TURN — it has user input awaiting your response.\n\
         Use Read(thread_id=\"{tid}\") to see the conversation and respond.",
    );
    let _r = SpineState::create_notification(
        &mut app.state,
        NotificationType::Custom,
        "my_turn_thread".to_string(),
        content,
    );
}

/// Extract thread IDs from notification content embedded in a synthetic message.
///
/// Looks for `thread_id="T..."` patterns (produced by thread input routing
/// and `check_my_turn_threads`). Returns all matches in order so the caller
/// can pick the first one that's still `MY_TURN`.
fn extract_thread_ids(content: &str) -> Vec<String> {
    let marker = "thread_id=\"";
    let mut ids = Vec::new();
    let mut search_from: usize = 0;
    while let Some(pos) = content.get(search_from..).and_then(|s| s.find(marker)) {
        let Some(start) = search_from.checked_add(pos).and_then(|v| v.checked_add(marker.len())) else {
            break;
        };
        if let Some(end_offset) = content.get(start..).and_then(|s| s.find('"')) {
            if let Some(id_str) = start.checked_add(end_offset).and_then(|end| content.get(start..end)) {
                ids.push(id_str.to_string());
            }
            search_from = start.saturating_add(end_offset).saturating_add(1);
        } else {
            break;
        }
    }
    ids
}
