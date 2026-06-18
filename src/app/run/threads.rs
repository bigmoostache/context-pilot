//! Thread-related helpers for the main event loop.
//!
//! Extracted from `lifecycle.rs` to keep it under the 500-line limit.
//! Contains auto-Read injection, `MY_TURN` thread detection, and
//! thread ID extraction from notification content.

use crate::app::App;
use crate::app::panels::now_ms;
use cp_base::state::data::message::{MsgKind, MsgStatus, ToolResultRecord, ToolUseRecord};
use cp_base::tools::ToolUse;
use cp_mod_bridge::BridgeState;
use cp_mod_spine::types::{NotificationType, SpineState};
use cp_mod_threads::types::{
    FocusState, ThreadAuthor, ThreadMessage, ThreadStatus, ThreadsState,
};
use cp_wire::types::command::{Command, Kind as CommandKind};

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

// ═══════════════════════════════════════════════════════════════════════
// Bridge command polling — accepts inbound commands from the backend and
// applies them on the main loop (K7 path).
// ═══════════════════════════════════════════════════════════════════════

use std::time::Duration;

/// Maximum time the main loop will wait for a single command connection to
/// finish reading.  A wedged commander is dropped after this window.
const READ_TIMEOUT: Duration = Duration::from_millis(500);

/// Poll the bridge listener for an inbound command connection and apply any
/// accepted commands.
///
/// Safe to call every tick: returns immediately when the bridge is OFF, the
/// listener is absent, or no connection is pending.
pub(super) fn poll_bridge_commands(app: &mut App) {
    let commands = accept_commands(&mut app.state);
    for cmd in commands {
        apply_command(app, cmd);
    }
}

/// Try to accept one connection and process it through the command intake.
///
/// Returns the (possibly empty) list of freshly-accepted [`Command`]s.
fn accept_commands(state: &mut cp_base::state::runtime::State) -> Vec<Command> {
    let bs = state.ext_mut::<BridgeState>();

    // Split borrows: &boot (for listener + oplog) and &mut intake.
    let (Some(boot), Some(intake)) = (&bs.boot, &mut bs.intake) else {
        return Vec::new();
    };

    // Non-blocking accept — returns WouldBlock when no connection is pending.
    let Ok((mut stream, _addr)) = boot.listener().accept() else {
        return Vec::new();
    };

    // Bound how long we wait for the commander to finish writing.
    let _ignored = stream.set_read_timeout(Some(READ_TIMEOUT));

    match intake.handle_connection(boot.oplog(), &mut stream) {
        Ok(cmds) => cmds,
        Err(e) => {
            log::error!("bridge: command intake error: {e:?}");
            Vec::new()
        }
    }
}

/// Dispatch a single accepted command to the appropriate agent action.
fn apply_command(app: &mut App, cmd: Command) {
    match cmd.kind {
        CommandKind::SendMessage { thread_id, content } => {
            apply_send_message(&mut app.state, &thread_id, &content);
        }
        CommandKind::CreateThread { name } => {
            apply_create_thread(&mut app.state, &name);
        }
        CommandKind::ArchiveThread { thread_id } => {
            apply_archive_thread(&mut app.state, &thread_id);
        }
        CommandKind::RestoreThread { thread_id } => {
            apply_restore_thread(&mut app.state, &thread_id);
        }
        CommandKind::Stop | CommandKind::InterruptStream => {
            apply_stop(&mut app.state);
        }
        CommandKind::Unknown => {
            log::warn!("bridge: ignoring unknown command {}", cmd.id);
        }
    }
}

// ── SendMessage (K7) ────────────────────────────────────────────────────

/// Inject a user message into the given thread and create a spine
/// notification so the agent attends to it.
///
/// This is the **K7 path**: commands enter the agent through the same
/// mechanism as local user input — a `ThreadMessage(User)` on the thread,
/// a `MyTurn` status flip, and a spine notification.
fn apply_send_message(
    state: &mut cp_base::state::runtime::State,
    thread_id: &str,
    content: &str,
) {
    let threads_state = ThreadsState::get_mut(state);
    let Some(thread) = threads_state.threads.iter_mut().find(|t| t.id == thread_id) else {
        log::warn!("bridge: SendMessage for unknown thread {thread_id}");
        return;
    };

    thread.messages.push(ThreadMessage {
        author: ThreadAuthor::User,
        content: Some(content.to_owned()),
        file_path: None,
        question: None,
        timestamp: now_ms(),
        acknowledged: false,
    });
    thread.status = ThreadStatus::MyTurn;

    // NO instant spine notification — the idle MY_TURN detection
    // (`check_my_turn_threads`) handles it when the agent finishes its
    // current work, avoiding mid-task distraction.

    for module in crate::modules::all_modules() {
        module.on_user_message(state);
    }

    state.flags.ui.dirty = true;
    log::info!("bridge: applied SendMessage on thread {thread_id}");
}

// ── CreateThread ────────────────────────────────────────────────────────

/// Create a new thread with the given name.
fn apply_create_thread(state: &mut cp_base::state::runtime::State, name: &str) {
    let ts = ThreadsState::get_mut(state);
    let id = format!("T{}", ts.next_id);
    ts.next_id = ts.next_id.saturating_add(1);

    ts.threads.push(cp_mod_threads::types::Thread {
        id: id.clone(),
        name: name.to_owned(),
        status: ThreadStatus::TheirTurn,
        messages: vec![],
        created_at: now_ms(),
        archived: false,
    });

    state.flags.ui.dirty = true;
    log::info!("bridge: created thread {id} \"{name}\"");
}

// ── ArchiveThread ───────────────────────────────────────────────────────

/// Mark the thread as archived (soft-delete).
fn apply_archive_thread(state: &mut cp_base::state::runtime::State, thread_id: &str) {
    let ts = ThreadsState::get_mut(state);
    let Some(thread) = ts.threads.iter_mut().find(|t| t.id == thread_id) else {
        log::warn!("bridge: ArchiveThread for unknown thread {thread_id}");
        return;
    };
    thread.archived = true;

    // Clean up focus references (mirrors archive_confirm in threads.rs).
    let focus = FocusState::get_mut(state);
    if focus.focused_thread_id.as_deref() == Some(thread_id) {
        focus.focused_thread_id = None;
        focus.dangling_remaining = 0;
        focus.escalation_level = 0;
    }
    let _prev = focus.last_read_count.remove(thread_id);
    if focus.notified_my_turn_id.as_deref() == Some(thread_id) {
        focus.notified_my_turn_id = None;
    }

    state.flags.ui.dirty = true;
    log::info!("bridge: archived thread {thread_id}");
}

// ── RestoreThread ───────────────────────────────────────────────────────

/// Restore an archived thread (clear the soft-delete flag).
fn apply_restore_thread(
    state: &mut cp_base::state::runtime::State,
    thread_id: &str,
) {
    let ts = ThreadsState::get_mut(state);
    if let Some(thread) = ts.threads.iter_mut().find(|t| t.id == thread_id) {
        thread.archived = false;
        state.flags.ui.dirty = true;
        log::info!("bridge: restored thread {thread_id}");
    } else {
        log::warn!("bridge: RestoreThread for unknown thread {thread_id}");
    }
}

// ── Stop / Interrupt ────────────────────────────────────────────────────

/// Stop the current stream (mirrors the Esc-key `StopStreaming` action).
fn apply_stop(state: &mut cp_base::state::runtime::State) {
    use cp_base::state::flags::StreamPhase;

    if state.flags.stream.phase.is_streaming() {
        state.flags.stream.phase.transition(StreamPhase::Idle);
        if let Some(ctx) = state
            .context
            .iter_mut()
            .find(|c| c.context_type.as_str() == cp_base::state::context::Kind::CONVERSATION)
        {
            ctx.token_count = ctx
                .token_count
                .saturating_sub(state.streaming_estimated_tokens);
        }
        state.streaming_estimated_tokens = 0;
        if let Some(msg) = state.messages.last_mut()
            && msg.role == "assistant" && !msg.content.is_empty()
        {
            msg.content.push_str("\n[Stopped]");
        }
        // Prevent spine from immediately relaunching.
        SpineState::get_mut(state).config.user_stopped = true;
        state.flags.ui.dirty = true;
        log::info!("bridge: stopped streaming");
    }

    // Notify modules (stream stop hooks).
    for module in crate::modules::all_modules() {
        module.on_stream_stop(state);
    }
}
