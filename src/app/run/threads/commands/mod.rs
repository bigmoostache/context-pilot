//! Bridge command *application* — the K7 intake path's mutation half.
//!
//! Split from the sibling [`bridge`](super::bridge) module (which had outgrown
//! the 500-line limit) so each file stays focused: `bridge` owns the socket
//! intake + live-state emission chokepoints, while this file owns the pure
//! state mutations a decoded [`Command`] applies —
//! `SendMessage`/`CreateThread`/`BranchThread`/`ArchiveThread`/`RestoreThread`/`Stop` — entered
//! exactly as local user input would be (the K7 path).

use cp_base::config::llm::models::{
    AnthropicModel, ClaudeCodeV2Model, DeepSeekModel, GrokModel, GroqModel, MiniMaxModel,
};
use cp_base::config::llm::openrouter_model::OpenRouterModel;
use cp_base::config::llm::types::LlmProvider;
use cp_base::state::runtime::State;
use cp_mod_bridge::BridgeState;
use cp_mod_spine::types::SpineState;
use cp_mod_threads::types::{FocusState, ThreadAuthor, ThreadMessage, ThreadStatus, ThreadsState};
use cp_wire::types::command::{Command, Kind as CommandKind};
use cp_wire::types::oplog::OpEntryKind;

use crate::app::App;

use self::create::{BranchPoint, Seed, apply_branch_thread, apply_create_thread};
use super::bridge::emit_roster_delta;

/// `CreateThread` / `BranchThread` application.
mod create;

/// Dispatch a single accepted command to the appropriate agent action.
pub(super) fn apply_command(app: &mut App, cmd: Command) {
    match cmd.kind {
        CommandKind::SendMessage { thread_id, content } => {
            if apply_send_message(&mut app.state, &thread_id, &content) {
                route_on_user_message(app, &thread_id);
            }
        }
        CommandKind::CreateThread { name, initial_message, paused } => {
            let seeded = initial_message.as_deref().is_some_and(|c| !c.trim().is_empty());
            let seed = Seed { initial_message: initial_message.as_deref(), paused };
            let id = apply_create_thread(&mut app.state, &name, &seed);
            if seeded {
                route_on_user_message(app, &id);
            }
        }
        CommandKind::BranchThread { source_thread_id, message_ts, name, initial_message, paused } => {
            let seeded = initial_message.as_deref().is_some_and(|c| !c.trim().is_empty());
            let point = BranchPoint { source_thread_id: &source_thread_id, message_ts };
            let seed = Seed { initial_message: initial_message.as_deref(), paused };
            let created = apply_branch_thread(&mut app.state, &point, &name, &seed);
            if let Some(id) = created.filter(|_| seeded) {
                route_on_user_message(app, &id);
            }
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
            app.teardown_thread(&thread_id);
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
        CommandKind::LoadBehaviour { id } => {
            apply_load_behaviour(&mut app.state, &id);
        }
        CommandKind::Unknown => {
            log::warn!("bridge: ignoring unknown command {}", cmd.id);
        }
    }
}

// ── SendMessage (K7) ────────────────────────────────────────────────────

/// Inject a user message into the given thread, flipping it to `MyTurn`.
///
/// This is the **K7 path**: commands enter the agent through the same
/// mechanism as local user input — a `ThreadMessage(User)` on the thread and a
/// `MyTurn` status flip (the per-thread dispatcher then nudges it into its own
/// spine inbox once it is schedulable).
///
/// Returns `true` if the message was applied (thread existed). The per-thread
/// lifecycle hook (`on_user_message`, which resets *that thread's* spine
/// counters — D4/S7) is NOT fired here: it needs [`App::deliver_to_thread`] to
/// target the owner thread's state rather than the executing thread's, so the caller
/// (`apply_command`) routes it after this returns. Firing it here on `state`
/// would reset the *executing* (focused) thread's counters for a message sent to
/// a *background* thread.
fn apply_send_message(state: &mut State, thread_id: &str, content: &str) -> bool {
    let threads_state = ThreadsState::get_mut(state);
    let Some(thread) = threads_state.threads.iter_mut().find(|t| t.id == thread_id) else {
        log::warn!("bridge: SendMessage for unknown thread {thread_id}");
        return false;
    };

    thread.messages.push(ThreadMessage::user(content.to_owned()));
    thread.status = ThreadStatus::MyTurn;

    state.flags.ui.dirty = true;
    log::info!("bridge: applied SendMessage on thread {thread_id}");
    true
}

/// Fire every module's `on_user_message` lifecycle hook against the owner
/// thread's state — the per-thread counter reset (D4/S7) — then re-engage the
/// thread if it was parked [`Errored`](cp_fleet::ThreadExecState::Errored).
///
/// Routed through [`App::deliver_to_thread`] so a message sent to a *background*
/// thread resets *that* thread's spine counters (auto-continuation count,
/// autonomous-start clock, `user_stopped`, error backoff), never the focused
/// executing's. At N=1 (or when the target IS the focused thread, or a
/// just-created thread not yet in the registry) `deliver_to_thread` runs the
/// hook directly on `state` — byte-identical to the former inline loop.
///
/// After the reset, [`App::clear_errored_entry`] flips a stuck (`Errored`)
/// thread back to `Runnable`: a fresh user message is the human-intervention
/// recovery path for a thread the loop had given up on (F4). A no-op for the
/// executing / unknown threads, so N=1 is unaffected.
fn route_on_user_message(app: &mut App, thread_id: &str) {
    app.deliver_to_thread(Some(thread_id), |state| {
        for module in crate::modules::all_modules() {
            module.on_user_message(state);
        }
    });
    app.clear_errored_entry(thread_id);
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
    }
    let _prev = focus.last_read_count.remove(thread_id);

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
    }
    let _prev = focus.last_read_count.remove(thread_id);

    emit_roster_delta(state, OpEntryKind::ThreadDeleted { thread_id: thread_id.to_owned() });

    // Thread-owned todos and scratchpad: hard-deleting a thread cascades removal
    // of its tasks (FR13) and scratchpad cells (archive keeps both, only
    // hard-delete cascades).
    let _todos = cp_mod_todo::tools::purge_thread_todos(state, thread_id);
    let _cells = cp_mod_scratchpad::tools::purge_thread_cells(state, thread_id);

    // Clean up all bridge memos for the deleted thread.
    if let Some(bs) = state.get_ext_mut::<BridgeState>() {
        let _status = bs.thread_statuses.remove(thread_id);
        let _archived = bs.thread_archived_memo.remove(thread_id);
        let _paused = bs.thread_paused_memo.remove(thread_id);
        let _msgs = bs.thread_msg_counts.remove(thread_id);
        let _tasks = bs.thread_tasks.remove(thread_id);
        let _notes = bs.thread_notes.remove(thread_id);
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

    let to_delete = collect_delete_timestamps(thread, idx, message_ts);

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
    log::info!("bridge: deleted {} message(s) from thread {thread_id} (target ts={message_ts})", to_delete.len());
}

/// Timestamps to delete for a `DeleteMessage`: the target plus, when the target
/// is an assistant message, all *consecutive* `auto:true` messages immediately
/// *preceding* it (tool-trace cleanup — stops at the first non-auto message).
fn collect_delete_timestamps(thread: &cp_mod_threads::types::Thread, idx: usize, message_ts: u64) -> Vec<u64> {
    let mut to_delete: Vec<u64> = vec![message_ts];
    let is_assistant = thread.messages.get(idx).is_some_and(|m| m.author == ThreadAuthor::Assistant);
    if !is_assistant {
        return to_delete;
    }
    if let Some(preceding) = thread.messages.get(..idx) {
        for msg in preceding.iter().rev() {
            if msg.auto {
                to_delete.push(msg.timestamp);
            } else {
                break;
            }
        }
    }
    to_delete
}

// ── Stop / Interrupt ────────────────────────────────────────────────────

/// Stop the current stream (mirrors the Esc-key `StopStreaming` action).
fn apply_stop(state: &mut State) {
    use cp_base::state::flags::StreamPhase;

    if state.thread().stream.phase.is_streaming() {
        state.thread_mut().stream.phase.transition(StreamPhase::Idle);
        let est = state.thread().streaming_estimated_tokens;
        if let Some(ctx) = state
            .thread_mut()
            .context
            .iter_mut()
            .find(|c| c.context_type.as_str() == cp_base::state::context::Kind::CONVERSATION)
        {
            ctx.token_count = ctx.token_count.saturating_sub(est);
        }
        state.thread_mut().streaming_estimated_tokens = 0;
        if let Some(msg) = state.thread_mut().messages.last_mut()
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
        LlmProvider::Anthropic | LlmProvider::ClaudeCodeApiKey => {
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
        LlmProvider::OpenRouter => {
            serde_json::from_value::<OpenRouterModel>(model_val).map(|m| state.openrouter_model = m).is_ok()
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

// ── LoadBehaviour (active behaviour agent) ─────────────────────────────

/// Switch the agent's active behaviour agent (prompt-library system prompt)
/// from the web footer selector. An empty `id` reverts to the default agent.
///
/// Routed through the shared [`cp_mod_prompt::tools::set_active_agent`] so the
/// bridge command and the local `agent_load` tool mutate the same
/// `PromptState.active_agent_id` through one path (no duplication). The touched
/// SYSTEM + LIBRARY panels re-render; the active flag surfaces to the web
/// footer on the next `library()` inspect read.
fn apply_load_behaviour(state: &mut State, id: &str) {
    match cp_mod_prompt::tools::set_active_agent(state, id) {
        Ok(name) => {
            state.flags.ui.dirty = true;
            log::info!("bridge: loaded behaviour agent {name} (id={id:?})");
        }
        Err(e) => log::warn!("bridge: LoadBehaviour failed: {e}"),
    }
}
