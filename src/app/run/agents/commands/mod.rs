//! Bridge command *application* — the K7 intake path's mutation half.
//!
//! Split from the sibling [`bridge`](super::bridge) module (which had outgrown
//! the 500-line limit) so each file stays focused: `bridge` owns the socket
//! intake + live-state emission chokepoints, while this file owns the pure
//! state mutations a decoded [`Command`] applies —
//! `SendMessage`/`CreateAgent`/`BranchAgent`/`ArchiveAgent`/`RestoreAgent`/`Stop` — entered
//! exactly as local user input would be (the K7 path).

use cp_base::config::llm::models::{
    AnthropicModel, ClaudeCodeV2Model, DeepSeekModel, GrokModel, GroqModel, MiniMaxModel,
};
use cp_base::config::llm::openrouter_model::OpenRouterModel;
use cp_base::config::llm::types::LlmProvider;
use cp_base::state::runtime::State;
use cp_mod_agents::types::{AgentAuthor, AgentMessage, AgentsState, FocusState};
use cp_mod_bridge::BridgeState;
use cp_mod_spine::types::SpineState;
use cp_wire::types::command::{Command, Kind as CommandKind};
use cp_wire::types::oplog::OpEntryKind;

use crate::app::App;

use self::create::{BranchPoint, Seed, apply_branch_agent, apply_create_agent};
use self::rename::apply_rename_agent;
use super::bridge::emit_roster_delta;

/// `CreateAgent` / `BranchAgent` application.
mod create;
mod rename;

/// Dispatch a single accepted command to the appropriate agent action.
pub(super) fn apply_command(app: &mut App, cmd: Command) {
    match cmd.kind {
        CommandKind::SendMessage { agent_id, content } => {
            if apply_send_message(&mut app.state, &agent_id, &content) {
                route_on_user_message(app, &agent_id);
            }
        }
        CommandKind::CreateAgent { name, initial_message, paused } => {
            let seeded = initial_message.as_deref().is_some_and(|c| !c.trim().is_empty());
            let seed = Seed { initial_message: initial_message.as_deref(), paused };
            let id = apply_create_agent(&mut app.state, &name, &seed);
            if seeded {
                route_on_user_message(app, &id);
            }
        }
        CommandKind::BranchAgent { source_agent_id, message_ts, name, initial_message, paused } => {
            let seeded = initial_message.as_deref().is_some_and(|c| !c.trim().is_empty());
            let point = BranchPoint { source_agent_id: &source_agent_id, message_ts };
            let seed = Seed { initial_message: initial_message.as_deref(), paused };
            let created = apply_branch_agent(&mut app.state, &point, &name, &seed);
            if let Some(id) = created.filter(|_| seeded) {
                route_on_user_message(app, &id);
            }
        }
        CommandKind::ArchiveAgent { agent_id } => {
            apply_archive_agent(&mut app.state, &agent_id);
        }
        CommandKind::RestoreAgent { agent_id } => {
            apply_restore_agent(&mut app.state, &agent_id);
        }
        CommandKind::PauseAgent { agent_id } => {
            apply_pause_agent(&mut app.state, &agent_id);
        }
        CommandKind::ResumeAgent { agent_id } => {
            apply_resume_agent(&mut app.state, &agent_id);
        }
        CommandKind::RenameAgent { agent_id, name } => apply_rename_agent(&mut app.state, &agent_id, &name),
        CommandKind::DeleteAgent { agent_id } => {
            apply_delete_agent(&mut app.state, &agent_id);
            app.teardown_agent(&agent_id);
        }
        CommandKind::DeleteMessage { agent_id, message_ts } => {
            apply_delete_message(&mut app.state, &agent_id, message_ts);
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

/// Inject a user message into the given agent, flipping it to `MyTurn`.
///
/// This is the **K7 path**: commands enter the agent through the same
/// mechanism as local user input — a `AgentMessage(User)` on the agent and a
/// `MyTurn` status flip (the per-agent dispatcher then nudges it into its own
/// spine inbox once it is schedulable).
///
/// Returns `true` if the message was applied (agent existed). The per-agent
/// lifecycle hook (`on_user_message`, which resets *that agent's* spine
/// counters — D4/S7) is NOT fired here: it needs [`App::deliver_to_agent`] to
/// target the owner agent's state rather than the executing agent's, so the caller
/// (`apply_command`) routes it after this returns. Firing it here on `state`
/// would reset the *executing* (focused) agent's counters for a message sent to
/// a *background* agent.
fn apply_send_message(state: &mut State, agent_id: &str, content: &str) -> bool {
    if !AgentsState::get(state).agents.iter().any(|t| t.id == agent_id) {
        log::warn!("bridge: SendMessage for unknown agent {agent_id}");
        return false;
    }

    cp_mod_agents::types::turn::give(state, agent_id);
    if let Some(history) = cp_mod_agents::types::msgs::messages_mut(state, agent_id) {
        history.push(AgentMessage::user(content.to_owned()));
    }

    state.flags.ui.dirty = true;
    log::info!("bridge: applied SendMessage on agent {agent_id}");
    true
}

/// Fire every module's `on_user_message` lifecycle hook against the owner
/// agent's state — the per-agent counter reset (D4/S7) — then re-engage the
/// agent if it was parked [`Errored`](cp_fleet::AgentExecState::Errored).
///
/// Routed through [`App::deliver_to_agent`] so a message sent to a *background*
/// agent resets *that* agent's spine counters (auto-continuation count,
/// autonomous-start clock, `user_stopped`, error backoff), never the focused
/// executing's. At N=1 (or when the target IS the focused agent, or a
/// just-created agent not yet in the registry) `deliver_to_agent` runs the
/// hook directly on `state` — byte-identical to the former inline loop.
///
/// After the reset, [`App::clear_errored_entry`] flips a stuck (`Errored`)
/// agent back to `Runnable`: a fresh user message is the human-intervention
/// recovery path for an agent the loop had given up on (F4). A no-op for the
/// executing / unknown threads, so N=1 is unaffected.
fn route_on_user_message(app: &mut App, agent_id: &str) {
    app.deliver_to_agent(Some(agent_id), |state| {
        for module in crate::modules::all_modules() {
            module.on_user_message(state);
        }
    });
    app.clear_errored_entry(agent_id);
}

// ── ArchiveAgent ───────────────────────────────────────────────────────

/// Mark the agent as archived (soft-delete).
fn apply_archive_agent(state: &mut State, agent_id: &str) {
    let ts = AgentsState::get_mut(state);
    let Some(agent) = ts.agents.iter_mut().find(|t| t.id == agent_id) else {
        log::warn!("bridge: ArchiveThread for unknown agent {agent_id}");
        return;
    };
    agent.archived = true;

    // Clean up focus references (mirrors archive_confirm in agents.rs).
    let focus = FocusState::get_mut(state);
    if focus.focused_agent_id.as_deref() == Some(agent_id) {
        focus.focused_agent_id = None;
    }
    let _prev = focus.last_read_count.remove(agent_id);
    if let Some(outcome) = cp_mod_git::worktree::release_agent(state, agent_id) {
        log::info!("bridge: archive {agent_id}: {outcome}");
    }

    emit_roster_delta(state, OpEntryKind::AgentArchived { agent_id: agent_id.to_owned() });
    if let Some(bs) = state.get_ext_mut::<BridgeState>() {
        let _inserted = bs.agent_archived_memo.insert(agent_id.to_owned(), true);
    }

    state.flags.ui.dirty = true;
    log::info!("bridge: archived agent {agent_id}");
}

// ── RestoreAgent ───────────────────────────────────────────────────────

/// Restore an archived agent (clear the soft-delete flag).
fn apply_restore_agent(state: &mut State, agent_id: &str) {
    let ts = AgentsState::get_mut(state);
    if let Some(agent) = ts.agents.iter_mut().find(|t| t.id == agent_id) {
        agent.archived = false;
        emit_roster_delta(state, OpEntryKind::AgentRestored { agent_id: agent_id.to_owned() });
        if let Some(bs) = state.get_ext_mut::<BridgeState>() {
            let _prev = bs.agent_archived_memo.insert(agent_id.to_owned(), false);
        }
        state.flags.ui.dirty = true;
        log::info!("bridge: restored agent {agent_id}");
    } else {
        log::warn!("bridge: RestoreThread for unknown agent {agent_id}");
    }
}

// ── PauseAgent ─────────────────────────────────────────────────────────

/// Pause an agent — suppress `MY_TURN` notifications without archiving.
fn apply_pause_agent(state: &mut State, agent_id: &str) {
    let ts = AgentsState::get_mut(state);
    if let Some(agent) = ts.agents.iter_mut().find(|t| t.id == agent_id) {
        agent.paused = true;
        emit_roster_delta(state, OpEntryKind::AgentPaused { agent_id: agent_id.to_owned() });
        if let Some(bs) = state.get_ext_mut::<BridgeState>() {
            let _prev = bs.agent_paused_memo.insert(agent_id.to_owned(), true);
        }
        state.flags.ui.dirty = true;
        log::info!("bridge: paused agent {agent_id}");
    } else {
        log::warn!("bridge: PauseThread for unknown agent {agent_id}");
    }
}

// ── ResumeAgent ────────────────────────────────────────────────────────

/// Resume a paused agent — re-enable `MY_TURN` notifications.
fn apply_resume_agent(state: &mut State, agent_id: &str) {
    let ts = AgentsState::get_mut(state);
    if let Some(agent) = ts.agents.iter_mut().find(|t| t.id == agent_id) {
        agent.paused = false;
        emit_roster_delta(state, OpEntryKind::AgentResumed { agent_id: agent_id.to_owned() });
        if let Some(bs) = state.get_ext_mut::<BridgeState>() {
            let _prev = bs.agent_paused_memo.insert(agent_id.to_owned(), false);
        }
        state.flags.ui.dirty = true;
        log::info!("bridge: resumed agent {agent_id}");
    } else {
        log::warn!("bridge: ResumeThread for unknown agent {agent_id}");
    }
}

// ── DeleteAgent ────────────────────────────────────────────────────────

/// Apply the `Agent` tool's queued effects (cp-management): announce created
/// sub-agents on the bridge, hard-delete doomed ones (deepest first) with
/// their runtimes.
pub(in crate::app::run) fn drain_management(app: &mut App) {
    let (created, doomed) = cp_management::types::ManagementState::drain(&mut app.state);
    for id in &created {
        let agent = AgentsState::get(&app.state).agents.iter().find(|a| a.id == *id);
        let name = agent.map(|a| a.name.clone()).unwrap_or_default();
        create::announce_agent(&mut app.state, id, &name, None);
    }
    for id in &doomed {
        {
            let _g = crate::profile!("apply_delete_agent");
            apply_delete_agent(&mut app.state, id);
        }
        let _g = crate::profile!("teardown_agent");
        app.teardown_agent(id);
    }
}

/// Delete the files of `removed` groups on a detached thread.
///
/// Falls back to inline removal only when the thread cannot be spawned.
fn remove_group_files_off_loop(removed: &[String]) {
    if removed.is_empty() {
        return;
    }
    let bg = removed.to_vec();
    let spawned = std::thread::Builder::new().name("group-file-rm".into()).spawn(move || {
        for gid in &bg {
            cp_groups::persist::remove_group_file(gid);
        }
    });
    if spawned.is_err() {
        for gid in removed {
            cp_groups::persist::remove_group_file(gid);
        }
    }
}

/// Permanently delete an agent and all its messages.
fn apply_delete_agent(state: &mut State, agent_id: &str) {
    let ts = AgentsState::get_mut(state);
    let existed = ts.agents.iter().any(|t| t.id == agent_id);
    if !existed {
        log::warn!("bridge: DeleteThread for unknown agent {agent_id}");
        return;
    }
    ts.agents.retain(|t| t.id != agent_id);
    // Groups it owns (its human group included) and coordination groups it sat
    // in go with their files; it leaves every other group.
    let removed =
        cp_groups::social::departed::purge(&mut cp_groups::types::GroupsState::get_mut(state).groups, &|m: &str| {
            m != agent_id
        });
    // File removal is IO the loop must not wait on.
    remove_group_files_off_loop(&removed);

    forget_agent_memos(state, agent_id);
    emit_roster_delta(state, OpEntryKind::AgentDeleted { agent_id: agent_id.to_owned() });

    // Thread-owned todos and scratchpad: hard-deleting an agent cascades removal
    // of its tasks (FR13) and scratchpad cells (archive keeps both, only
    // hard-delete cascades).
    let _todos = cp_mod_todo::tools::purge_agent_todos(state, agent_id);
    let _cells = cp_mod_scratchpad::tools::purge_agent_cells(state, agent_id);

    state.flags.ui.dirty = true;
    log::info!("bridge: permanently deleted agent {agent_id}");
}

/// Drop the focus references and bridge memos a deleted agent leaves behind
/// (focus mirrors the archive path).
fn forget_agent_memos(state: &mut State, agent_id: &str) {
    let focus = FocusState::get_mut(state);
    if focus.focused_agent_id.as_deref() == Some(agent_id) {
        focus.focused_agent_id = None;
    }
    let _prev = focus.last_read_count.remove(agent_id);

    if let Some(bs) = state.get_ext_mut::<BridgeState>() {
        let _status = bs.agent_statuses.remove(agent_id);
        let _archived = bs.agent_archived_memo.remove(agent_id);
        let _paused = bs.agent_paused_memo.remove(agent_id);
        let _msgs = bs.agent_msg_counts.remove(agent_id);
        let _activity = bs.agent_activity_counts.remove(agent_id);
        let _tasks = bs.agent_tasks.remove(agent_id);
        let _notes = bs.agent_notes.remove(agent_id);
    }
}

// ── DeleteMessage ───────────────────────────────────────────────────

/// Delete a single message from an agent, identified by its epoch-ms
/// timestamp (unique within an agent).
///
/// **Cascade rule:** when the deleted message is from the assistant,
/// all *consecutive* `auto: true` messages immediately *preceding* it are
/// also removed (tool-trace cleanup — these are the tool calls that
/// produced the response). The cascade stops at the first non-auto
/// message. One `MessageDeleted` delta is emitted per removed message so
/// the frontend reducer handles each independently.
fn apply_delete_message(state: &mut State, agent_id: &str, message_ts: u64) {
    // Traces live in the agent's activity, messages in its human group: the
    // cascade runs over the interleaved timeline, then prunes both stores.
    let timeline: Vec<AgentMessage> =
        cp_mod_agents::types::activity::timeline(state, agent_id).into_iter().cloned().collect();
    let Some(idx) = timeline.iter().position(|m| m.timestamp == message_ts) else {
        log::warn!("bridge: DeleteMessage no message with ts={message_ts} in agent {agent_id}");
        return;
    };
    let to_delete = collect_delete_timestamps(&timeline, idx, message_ts);
    let delete_set: std::collections::HashSet<u64> = to_delete.iter().copied().collect();

    let Some(history) = cp_mod_agents::types::msgs::messages_mut(state, agent_id) else {
        log::warn!("bridge: DeleteMessage for unknown agent {agent_id}");
        return;
    };
    history.retain(|m| !delete_set.contains(&m.timestamp));
    let new_count = history.len();
    let activity_count =
        cp_mod_agents::types::activity::retain(state, agent_id, |m| !delete_set.contains(&m.timestamp));

    // Emit one delta per deleted message.
    let tid = agent_id.to_owned();
    for &ts_val in &to_delete {
        emit_roster_delta(state, OpEntryKind::MessageDeleted { agent_id: tid.clone(), message_ts: ts_val });
    }

    // Update the bridge's message-count memo so `emit_messages` sees the
    // reduced count and correctly emits `MessageCreated` for any subsequent
    // append (T418 fix).
    if let Some(bs) = state.get_ext_mut::<BridgeState>() {
        let _prev_traces = bs.agent_activity_counts.insert(tid.clone(), activity_count);
        let _prev_msgs = bs.agent_msg_counts.insert(tid, new_count);
    }

    state.flags.ui.dirty = true;
    log::info!("bridge: deleted {} message(s) from agent {agent_id} (target ts={message_ts})", to_delete.len());
}

/// Timestamps to delete for a `DeleteMessage`: the target plus, when the target
/// is an assistant message, all *consecutive* `auto:true` messages immediately
/// *preceding* it (tool-trace cleanup — stops at the first non-auto message).
fn collect_delete_timestamps(history: &[AgentMessage], idx: usize, message_ts: u64) -> Vec<u64> {
    let mut to_delete: Vec<u64> = vec![message_ts];
    let is_assistant = history.get(idx).is_some_and(|m| m.author == AgentAuthor::Assistant);
    if !is_assistant {
        return to_delete;
    }
    if let Some(preceding) = history.get(..idx) {
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

    if state.agent().stream.phase.is_streaming() {
        state.agent_mut().stream.phase.transition(StreamPhase::Idle);
        let est = state.agent().streaming_estimated_tokens;
        if let Some(ctx) = state
            .agent_mut()
            .context
            .iter_mut()
            .find(|c| c.context_type.as_str() == cp_base::state::context::Kind::CONVERSATION)
        {
            ctx.token_count = ctx.token_count.saturating_sub(est);
        }
        state.agent_mut().streaming_estimated_tokens = 0;
        if let Some(msg) = state.agent_mut().messages.last_mut()
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
/// Routed through the shared [`cp_mod_prompt::tools::set_active_system`] so the
/// bridge command and the local `system_load` tool mutate the same
/// `PromptState.active_system_id` through one path (no duplication). The touched
/// SYSTEM + LIBRARY panels re-render; the active flag surfaces to the web
/// footer on the next `library()` inspect read.
fn apply_load_behaviour(state: &mut State, id: &str) {
    match cp_mod_prompt::tools::set_active_system(state, id) {
        Ok(name) => {
            state.flags.ui.dirty = true;
            log::info!("bridge: loaded behaviour agent {name} (id={id:?})");
        }
        Err(e) => log::warn!("bridge: LoadBehaviour failed: {e}"),
    }
}
