//! Bridge command *application* — the K7 intake path's mutation half.
//!
//! Split from the sibling [`bridge`](super::bridge) module (which had outgrown
//! the 500-line limit) so each file stays focused: `bridge` owns the socket
//! intake + live-state emission chokepoints, while this file owns the pure
//! state mutations a decoded [`Command`] applies —
//! `SendMessage`/`CreateThread`/`ArchiveThread`/`RestoreThread`/`Stop` — entered
//! exactly as local user input would be (the K7 path).

use cp_base::config::llm_types::LlmProvider;
use cp_base::config::models::{AnthropicModel, ClaudeCodeV2Model, DeepSeekModel, GrokModel, GroqModel, MiniMaxModel};
use cp_base::state::runtime::State;
use cp_mod_bridge::BridgeState;
use cp_mod_spine::types::{NotificationType, SpineState};
use cp_mod_threads::types::{FocusState, ThreadAuthor, ThreadMessage, ThreadStatus, ThreadsState};
use cp_wire::types::command::{Command, Kind as CommandKind};
use cp_wire::types::oplog::OpEntryKind;

use crate::app::App;
use crate::app::panels::now_ms;

use super::bridge::{emit_roster_delta, wire_turn};

/// Dispatch a single accepted command to the appropriate agent action.
pub(super) fn apply_command(app: &mut App, cmd: Command) {
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
        CommandKind::PauseThread { thread_id } => {
            apply_pause_thread(&mut app.state, &thread_id);
        }
        CommandKind::ResumeThread { thread_id } => {
            apply_resume_thread(&mut app.state, &thread_id);
        }
        CommandKind::DeleteThread { thread_id } => {
            apply_delete_thread(&mut app.state, &thread_id);
        }
        CommandKind::DeleteMessage { thread_id, message_ts } => {
            apply_delete_message(&mut app.state, &thread_id, message_ts);
        }
        CommandKind::Stop | CommandKind::InterruptStream => {
            apply_stop(&mut app.state);
        }
        CommandKind::Configure { provider, model } => {
            apply_configure(&mut app.state, &provider, &model);
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
fn apply_send_message(state: &mut State, thread_id: &str, content: &str) {
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
        auto: false,
    });
    thread.status = ThreadStatus::MyTurn;
    let thread_name = thread.name.clone();

    // No instant spine notification for unfocused threads — the idle
    // MY_TURN detection (`check_my_turn_threads`) handles it when the
    // agent finishes, avoiding mid-task distraction.
    //
    // Exception: if the message lands on the CURRENTLY FOCUSED thread,
    // fire immediately — the user is actively talking to the agent in the
    // thread it's working on and expects a quick acknowledgement.
    if FocusState::get(state).focused_thread_id.as_deref() == Some(thread_id) {
        let notif = format!(
            "The user has just sent a NEW message to your currently focused thread \
             \"{thread_name}\" ({thread_id}):\n\n\
             {content}\n\n\
             Please acknowledge QUICKLY:\n\
             1. Read(thread_id=\"{thread_id}\") to refresh the conversation\n\
             2. Send a short acknowledgement (still_my_turn=true)",
        );
        let _r = SpineState::create_notification(
            state,
            NotificationType::Custom,
            "focused_thread_input".to_string(),
            notif,
        );
    }

    for module in crate::modules::all_modules() {
        module.on_user_message(state);
    }

    state.flags.ui.dirty = true;
    log::info!("bridge: applied SendMessage on thread {thread_id}");
}

// ── CreateThread ────────────────────────────────────────────────────────

/// Create a new thread with the given name.
fn apply_create_thread(state: &mut State, name: &str) {
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
        paused: false,
    });

    // Emit the durable roster delta so the backend view reflects the new
    // thread in ms (Leg 0 keystone) — a fresh, empty thread is the user's turn
    // (they must type the first message). Routed through `wire_turn` so the
    // emitted status matches what the status chokepoint would emit, and the
    // status memo is primed to this value so creating then immediately sending
    // a message produces exactly one follow-up `ThreadStatusChanged`.
    let created_turn = wire_turn(ThreadStatus::TheirTurn);
    emit_roster_delta(
        state,
        OpEntryKind::ThreadCreated {
            thread_id: id.clone(),
            name: name.to_owned(),
            status: created_turn,
            timestamp_ms: now_ms(),
        },
    );
    if let Some(bs) = state.get_ext_mut::<BridgeState>() {
        let _prev = bs.thread_statuses.insert(id.clone(), created_turn);
    }

    state.flags.ui.dirty = true;
    log::info!("bridge: created thread {id} \"{name}\"");
}

// ── ArchiveThread ───────────────────────────────────────────────────────

/// Mark the thread as archived (soft-delete).
fn apply_archive_thread(state: &mut State, thread_id: &str) {
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

    emit_roster_delta(state, OpEntryKind::ThreadArchived { thread_id: thread_id.to_owned() });
    if let Some(bs) = state.get_ext_mut::<BridgeState>() {
        let _inserted = bs.thread_archived_memo.insert(thread_id.to_owned(), true);
    }

    state.flags.ui.dirty = true;
    log::info!("bridge: archived thread {thread_id}");
}

// ── RestoreThread ───────────────────────────────────────────────────────

/// Restore an archived thread (clear the soft-delete flag).
fn apply_restore_thread(state: &mut State, thread_id: &str) {
    let ts = ThreadsState::get_mut(state);
    if let Some(thread) = ts.threads.iter_mut().find(|t| t.id == thread_id) {
        thread.archived = false;
        emit_roster_delta(state, OpEntryKind::ThreadRestored { thread_id: thread_id.to_owned() });
        if let Some(bs) = state.get_ext_mut::<BridgeState>() {
            let _prev = bs.thread_archived_memo.insert(thread_id.to_owned(), false);
        }
        state.flags.ui.dirty = true;
        log::info!("bridge: restored thread {thread_id}");
    } else {
        log::warn!("bridge: RestoreThread for unknown thread {thread_id}");
    }
}

// ── PauseThread ─────────────────────────────────────────────────────────

/// Pause a thread — suppress `MY_TURN` notifications without archiving.
fn apply_pause_thread(state: &mut State, thread_id: &str) {
    let ts = ThreadsState::get_mut(state);
    if let Some(thread) = ts.threads.iter_mut().find(|t| t.id == thread_id) {
        thread.paused = true;
        emit_roster_delta(state, OpEntryKind::ThreadPaused { thread_id: thread_id.to_owned() });
        if let Some(bs) = state.get_ext_mut::<BridgeState>() {
            let _prev = bs.thread_paused_memo.insert(thread_id.to_owned(), true);
        }
        state.flags.ui.dirty = true;
        log::info!("bridge: paused thread {thread_id}");
    } else {
        log::warn!("bridge: PauseThread for unknown thread {thread_id}");
    }
}

// ── ResumeThread ────────────────────────────────────────────────────────

/// Resume a paused thread — re-enable `MY_TURN` notifications.
fn apply_resume_thread(state: &mut State, thread_id: &str) {
    let ts = ThreadsState::get_mut(state);
    if let Some(thread) = ts.threads.iter_mut().find(|t| t.id == thread_id) {
        thread.paused = false;
        emit_roster_delta(state, OpEntryKind::ThreadResumed { thread_id: thread_id.to_owned() });
        if let Some(bs) = state.get_ext_mut::<BridgeState>() {
            let _prev = bs.thread_paused_memo.insert(thread_id.to_owned(), false);
        }
        state.flags.ui.dirty = true;
        log::info!("bridge: resumed thread {thread_id}");
    } else {
        log::warn!("bridge: ResumeThread for unknown thread {thread_id}");
    }
}

// ── DeleteThread ────────────────────────────────────────────────────────

/// Permanently delete a thread and all its messages.
fn apply_delete_thread(state: &mut State, thread_id: &str) {
    let ts = ThreadsState::get_mut(state);
    let existed = ts.threads.iter().any(|t| t.id == thread_id);
    if !existed {
        log::warn!("bridge: DeleteThread for unknown thread {thread_id}");
        return;
    }
    ts.threads.retain(|t| t.id != thread_id);

    // Clean up focus references (mirrors archive path).
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

    emit_roster_delta(state, OpEntryKind::ThreadDeleted { thread_id: thread_id.to_owned() });

    // Clean up all bridge memos for the deleted thread.
    if let Some(bs) = state.get_ext_mut::<BridgeState>() {
        let _status = bs.thread_statuses.remove(thread_id);
        let _archived = bs.thread_archived_memo.remove(thread_id);
        let _paused = bs.thread_paused_memo.remove(thread_id);
        let _msgs = bs.thread_msg_counts.remove(thread_id);
    }

    state.flags.ui.dirty = true;
    log::info!("bridge: permanently deleted thread {thread_id}");
}

// ── DeleteMessage ───────────────────────────────────────────────────

/// Delete a single message from a thread, identified by its epoch-ms
/// timestamp (unique within a thread).
///
/// **Cascade rule:** when the deleted message is from the assistant,
/// all *consecutive* `auto: true` messages immediately *preceding* it are
/// also removed (tool-trace cleanup — these are the tool calls that
/// produced the response). The cascade stops at the first non-auto
/// message. One `MessageDeleted` delta is emitted per removed message so
/// the frontend reducer handles each independently.
fn apply_delete_message(state: &mut State, thread_id: &str, message_ts: u64) {
    let ts = ThreadsState::get_mut(state);
    let Some(thread) = ts.threads.iter_mut().find(|t| t.id == thread_id) else {
        log::warn!("bridge: DeleteMessage for unknown thread {thread_id}");
        return;
    };

    // Find the target message index.
    let Some(idx) = thread.messages.iter().position(|m| m.timestamp == message_ts) else {
        log::warn!("bridge: DeleteMessage no message with ts={message_ts} in thread {thread_id}");
        return;
    };

    let is_assistant = thread.messages.get(idx).is_some_and(|m| m.author == ThreadAuthor::Assistant);

    // Collect timestamps to delete: the target + any trailing auto messages.
    let mut to_delete: Vec<u64> = vec![message_ts];

    if is_assistant {
        // Walk backward from idx-1 collecting consecutive auto messages
        // (tool-call traces that produced this response).
        if let Some(preceding) = thread.messages.get(..idx) {
            for msg in preceding.iter().rev() {
                if msg.auto {
                    to_delete.push(msg.timestamp);
                } else {
                    break;
                }
            }
        }
    }

    // Remove all collected messages.
    let delete_set: std::collections::HashSet<u64> = to_delete.iter().copied().collect();
    thread.messages.retain(|m| !delete_set.contains(&m.timestamp));
    let new_count = thread.messages.len();

    // Emit one delta per deleted message.
    let tid = thread_id.to_owned();
    for &ts_val in &to_delete {
        emit_roster_delta(state, OpEntryKind::MessageDeleted { thread_id: tid.clone(), message_ts: ts_val });
    }

    // Update the bridge's message-count memo so `emit_messages` sees the
    // reduced count and correctly emits `MessageCreated` for any subsequent
    // append (T418 fix).
    if let Some(bs) = state.get_ext_mut::<BridgeState>() {
        let _prev = bs.thread_msg_counts.insert(tid, new_count);
    }

    state.flags.ui.dirty = true;
    log::info!("bridge: deleted {} message(s) from thread {thread_id} (target ts={message_ts})", to_delete.len(),);
}

// ── Stop / Interrupt ────────────────────────────────────────────────────

/// Stop the current stream (mirrors the Esc-key `StopStreaming` action).
fn apply_stop(state: &mut State) {
    use cp_base::state::flags::StreamPhase;

    if state.flags.stream.phase.is_streaming() {
        state.flags.stream.phase.transition(StreamPhase::Idle);
        if let Some(ctx) =
            state.context.iter_mut().find(|c| c.context_type.as_str() == cp_base::state::context::Kind::CONVERSATION)
        {
            ctx.token_count = ctx.token_count.saturating_sub(state.streaming_estimated_tokens);
        }
        state.streaming_estimated_tokens = 0;
        if let Some(msg) = state.messages.last_mut()
            && msg.role == "assistant"
            && !msg.content.is_empty()
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

// ── Configure (LLM provider + model) ───────────────────────────────────

/// Apply a provider+model change from the web frontend.
///
/// Both strings use the serde names from [`LlmProvider`] (lowercase) and
/// the per-provider model enums (kebab-case). Invalid names are logged and
/// ignored — the agent keeps its current config.
fn apply_configure(state: &mut State, provider_str: &str, model_str: &str) {
    let provider_val = serde_json::Value::String(provider_str.to_owned());
    let Ok(provider) = serde_json::from_value::<LlmProvider>(provider_val) else {
        log::warn!("bridge: Configure unknown provider \"{provider_str}\"");
        return;
    };

    let model_val = serde_json::Value::String(model_str.to_owned());
    let model_ok = match provider {
        LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
            serde_json::from_value::<AnthropicModel>(model_val).map(|m| state.anthropic_model = m).is_ok()
        }
        LlmProvider::ClaudeCodeV2 => {
            serde_json::from_value::<ClaudeCodeV2Model>(model_val).map(|m| state.claude_code_v2_model = m).is_ok()
        }
        LlmProvider::Grok => serde_json::from_value::<GrokModel>(model_val).map(|m| state.grok_model = m).is_ok(),
        LlmProvider::Groq => serde_json::from_value::<GroqModel>(model_val).map(|m| state.groq_model = m).is_ok(),
        LlmProvider::DeepSeek => {
            serde_json::from_value::<DeepSeekModel>(model_val).map(|m| state.deepseek_model = m).is_ok()
        }
        LlmProvider::MiniMax => {
            serde_json::from_value::<MiniMaxModel>(model_val).map(|m| state.minimax_model = m).is_ok()
        }
    };

    if !model_ok {
        log::warn!("bridge: Configure unknown model \"{model_str}\" for provider \"{provider_str}\"");
        return;
    }

    state.llm_provider = provider;
    state.flags.ui.dirty = true;
    log::info!("bridge: configured provider={provider_str} model={model_str}");
}
