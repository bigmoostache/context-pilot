//! Bridge command intake + live-state emission for the main event loop.
//!
//! Two responsibilities, both gated on the orchestration bridge being ON:
//!
//! 1. **Command intake (K7 path).** [`poll_bridge_commands`] accepts inbound
//!    commands from the backend over the bridge socket and applies them on the
//!    main loop — `SendMessage`/`CreateThread`/`ArchiveThread`/`RestoreThread`
//!    enter the agent exactly as local user input would, and `Stop`/`Interrupt`
//!    mirror the Esc key.
//! 2. **Live-state emission (Leg 0 keystone).** The instant a mutation applies,
//!    a rev-numbered oplog delta is appended so the backend's in-memory view —
//!    and thus the web frontend — learns of it in milliseconds instead of
//!    waiting on the debounced tier-② disk write. Thread-roster deltas ride the
//!    non-blocking *durable* path ([`emit_roster_delta`]); the disposable,
//!    self-healing live vitals (phase + cost) ride the *best-effort* path
//!    ([`emit_vitals`]).
//!
//! Extracted from the parent `threads` module so each file stays under the
//! 500-line limit.

use std::time::Duration;

use cp_base::state::runtime::State;
use cp_mod_bridge::BridgeState;
use cp_mod_spine::types::SpineState;
use cp_mod_threads::types::{FocusState, ThreadAuthor, ThreadMessage, ThreadStatus, ThreadsState};
use cp_wire::types::command::{Command, Kind as CommandKind};
use cp_wire::types::oplog::OpEntryKind;
use cp_wire::types::{Phase, ThreadTurn};

use crate::app::App;
use crate::app::panels::now_ms;

// ═══════════════════════════════════════════════════════════════════════
// Bridge command polling — accepts inbound commands from the backend and
// applies them on the main loop (K7 path).
// ═══════════════════════════════════════════════════════════════════════

/// Maximum time the main loop will wait for a single command connection to
/// finish reading.  A wedged commander is dropped after this window.
const READ_TIMEOUT: Duration = Duration::from_millis(500);

/// Whether the orchestration bridge is active (booted) for this agent.
///
/// Cheap `TypeMap` lookup used by the main loop to decide its idle poll
/// cadence: when a web UI is connected through the bridge, the loop services
/// the command socket every couple of ms so a web command applies in single-
/// digit ms; otherwise it idles slowly to save CPU.
pub(in crate::app::run) fn bridge_active(state: &State) -> bool {
    state.get_ext::<BridgeState>().is_some_and(|bs| bs.boot.is_some())
}

/// Poll the bridge listener for inbound command connections and apply every
/// accepted command.
///
/// Drains **all** currently-pending connections each tick (bounded by
/// [`DRAIN_BUDGET`]) rather than one per loop iteration, so a burst of web
/// commands applies in the same tick instead of trickling in one-per-tick
/// (design doc Phase 3.1). Safe to call every tick: returns immediately when
/// the bridge is OFF, the listener is absent, or no connection is pending.
pub(in crate::app::run) fn poll_bridge_commands(app: &mut App) {
    let mut budget = DRAIN_BUDGET;
    while budget > 0 {
        budget = budget.saturating_sub(1);
        let Some(commands) = accept_commands(&mut app.state) else {
            break; // no pending connection — done draining this tick.
        };
        for cmd in commands {
            apply_command(app, cmd);
        }
    }
}

/// Upper bound on connections drained per tick — guards the drain loop against
/// a misbehaving flood while still clearing any realistic command burst.
const DRAIN_BUDGET: u32 = 64;

/// Try to accept one pending connection and process it through the command
/// intake.
///
/// Returns `None` when no connection is pending (the bridge is OFF, the
/// listener is absent, or `accept` would block) — the signal to stop draining.
/// Returns `Some(cmds)` when a connection was handled, even if it yielded no
/// accepted commands (an errored connection still counts, so draining
/// continues to the next pending one).
fn accept_commands(state: &mut State) -> Option<Vec<Command>> {
    let bs = state.ext_mut::<BridgeState>();

    // Split borrows: &boot (for listener + oplog) and &mut intake.
    let (Some(boot), Some(intake)) = (&bs.boot, &mut bs.intake) else {
        return None;
    };

    // Non-blocking accept — WouldBlock means no connection is pending.
    let Ok((mut stream, _addr)) = boot.listener().accept() else {
        return None;
    };

    // Bound how long we wait for the commander to finish writing.
    let _ignored = stream.set_read_timeout(Some(READ_TIMEOUT));

    match intake.handle_connection(boot.oplog(), &mut stream) {
        Ok(cmds) => Some(cmds),
        Err(e) => {
            log::error!("bridge: command intake error: {e:?}");
            Some(Vec::new())
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

// ── Roster delta emission (Leg 0 keystone — design doc I8/I10) ───────────

/// Append a thread-roster oplog delta the instant a mutation applies, so the
/// backend's in-memory view (and thus the web frontend) learns of it in
/// milliseconds instead of waiting on the debounced tier-② disk write.
///
/// No-op when the bridge is OFF (no `BridgeState.boot`). Uses the **non-blocking
/// durable** path ([`submit_durable`](cp_oplog::service::Service::submit_durable)):
/// the record is group-committed + `fdatasync`'d off-loop by the oplog thread,
/// so the main loop never blocks on a sync (design doc I2) yet the delta is
/// never dropped (a created/archived thread cannot be silently lost).
///
/// [`submit_durable`]: cp_oplog::service::Service::submit_durable
fn emit_roster_delta(state: &State, kind: OpEntryKind) {
    if let Some(bs) = state.get_ext::<BridgeState>()
        && let Some(boot) = bs.boot.as_ref()
    {
        boot.oplog().submit_durable(kind);
    }
}

// ── Live-vitals emission: phase + cost (Phase 2.3/2.4 — design doc I8/§15) ─

/// Map the agent's internal [`StreamPhase`] to the wire [`Phase`] observers see.
///
/// The internal machine distinguishes *receiving tokens* from *executing tools*
/// (both are "streaming" locally); the wire exposes that distinction directly so
/// the UI can render `streaming` vs `tooling` vs `idle`.
///
/// [`StreamPhase`]: cp_base::state::flags::StreamPhase
const fn wire_phase(phase: cp_base::state::flags::StreamPhase) -> Phase {
    use cp_base::state::flags::StreamPhase;
    match phase {
        StreamPhase::Receiving => Phase::Streaming,
        StreamPhase::ExecutingTools => Phase::Tooling,
        StreamPhase::Idle => Phase::Idle,
    }
}

/// Append `kind` to the oplog via the **best-effort** path (drop-on-full, never
/// blocks the loop). No-op when the bridge is OFF.
///
/// Used for the disposable, self-healing live-vitals records
/// ([`PhaseTransition`](OpEntryKind::PhaseTransition) /
/// [`CostAggregate`](OpEntryKind::CostAggregate)): a lost intermediate phase is
/// reconstructed on replay and a dropped cost sample re-aggregates, so unlike
/// the roster deltas these must never stall the main loop for durability (I2).
fn emit_best_effort(state: &State, kind: OpEntryKind) {
    if let Some(bs) = state.get_ext::<BridgeState>()
        && let Some(boot) = bs.boot.as_ref()
    {
        let _outcome = boot.oplog().append_best_effort(kind);
    }
}

/// Emit a [`PhaseTransition`](OpEntryKind::PhaseTransition) and/or
/// [`CostAggregate`](OpEntryKind::CostAggregate) the instant either changes, so
/// the backend view (and the web UI) reflect live LLM vitals in milliseconds.
///
/// Called every main-loop tick — the **single chokepoint** for vitals emission
/// (in contrast to scattering across stream hooks): it reads the authoritative
/// in-memory state, compares against the last-emitted value held in
/// [`BridgeState`], and emits **only on change**, so an idle loop and an
/// unchanged-cost stream add zero oplog traffic. Both records are best-effort
/// (I2): emission can never block or stall a tick.
///
/// No-op when the bridge is OFF (the `bridge_active` guard short-circuits).
pub(in crate::app::run) fn emit_vitals(app: &mut App) {
    use cp_base::cast::Safe as _;

    if !bridge_active(&app.state) {
        return;
    }

    // Phase — emit on transition only.
    let phase = wire_phase(app.state.flags.stream.phase);
    let phase_changed = app
        .state
        .get_ext::<BridgeState>()
        .is_some_and(|bs| bs.last_phase != Some(phase));
    if phase_changed {
        emit_best_effort(&app.state, OpEntryKind::PhaseTransition { phase });
        app.state.ext_mut::<BridgeState>().last_phase = Some(phase);
    }

    // Cost — cumulative-since-boot; emit when the dollar total moves.
    let cost_usd = app.state.cost_hit_usd + app.state.cost_miss_usd + app.state.cost_output_usd;
    let cost_changed = app
        .state
        .get_ext::<BridgeState>()
        .is_some_and(|bs| (bs.last_cost_usd - cost_usd).abs() > f64::EPSILON);
    if cost_changed {
        let input_tokens =
            app.state.cache_hit_tokens.to_u64().saturating_add(app.state.cache_miss_tokens.to_u64());
        let output_tokens = app.state.total_output_tokens.to_u64();
        emit_best_effort(&app.state, OpEntryKind::CostAggregate { input_tokens, output_tokens, cost_usd });
        app.state.ext_mut::<BridgeState>().last_cost_usd = cost_usd;
    }
}

// ── Thread status emission (Phase 1.4 status_changed — design doc I8) ─────

/// Map the agent's [`ThreadStatus`] to the wire [`ThreadTurn`] observers see.
///
/// The mapping is **identity by name** — `MyTurn → MyTurn`, `TheirTurn →
/// TheirTurn` — matching the disk-plane reshape
/// (`transport/rest/thread_shape.rs`), so a status served from a tier-② disk
/// read and one carried by a live delta resolve to the *same* web bucket. (The
/// wire enum's own doc comments describe the human-centric reading; the data
/// convention actually in use across both planes is identity-by-name, and this
/// helper is the single place that conversion lives.)
const fn wire_turn(status: ThreadStatus) -> ThreadTurn {
    match status {
        ThreadStatus::MyTurn => ThreadTurn::MyTurn,
        ThreadStatus::TheirTurn => ThreadTurn::TheirTurn,
    }
}

/// Emit a [`ThreadStatusChanged`](OpEntryKind::ThreadStatusChanged) the instant
/// any thread's turn-status flips, so the backend view (and the web roster)
/// move the thread to the right bucket in milliseconds instead of waiting on
/// the debounced tier-② disk write.
///
/// Like [`emit_messages`](super::messages::emit_messages), this is a main-loop
/// **observe-on-change chokepoint**: it diffs each thread's live status against
/// the per-thread snapshot held in [`BridgeState`] and emits **only on an
/// actual flip**, so it captures a transition from *every* source — a web
/// `SendMessage`, the agent's `Send` tool, a TUI reply, the agent finishing a
/// turn — with one uniform path rather than an emit call scattered at each
/// mutation site. A status flip is user-visible roster state, so it rides the
/// **durable** (never-dropped, never-loop-blocking) path
/// ([`emit_roster_delta`]).
///
/// The first pass after boot **seeds** the snapshot without emitting, so a
/// (re)started agent does not replay every thread's status as a spurious
/// change (the cold roster rides the frontend's initial tier-② load).
///
/// No-op when the bridge is OFF.
pub(in crate::app::run) fn emit_thread_status(app: &mut App) {
    if !bridge_active(&app.state) {
        return;
    }

    // First pass: snapshot existing statuses without emitting.
    let seeded = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.status_memo_seeded);
    if !seeded {
        let statuses: Vec<(String, ThreadTurn)> = ThreadsState::get(&app.state)
            .threads
            .iter()
            .map(|t| (t.id.clone(), wire_turn(t.status)))
            .collect();
        let bs = app.state.ext_mut::<BridgeState>();
        for (id, turn) in statuses {
            let _prev = bs.thread_statuses.insert(id, turn);
        }
        bs.status_memo_seeded = true;
        return;
    }

    // Diff live statuses against the memo; collect the flips (owned, so the
    // borrows end before we emit + update the memo below).
    let changed: Vec<(String, ThreadTurn)> = {
        let ts = ThreadsState::get(&app.state);
        let memo = &app.state.ext::<BridgeState>().thread_statuses;
        ts.threads
            .iter()
            .filter_map(|t| {
                let turn = wire_turn(t.status);
                (memo.get(&t.id).copied() != Some(turn)).then(|| (t.id.clone(), turn))
            })
            .collect()
    };

    for (thread_id, status) in changed {
        emit_roster_delta(
            &app.state,
            OpEntryKind::ThreadStatusChanged { thread_id: thread_id.clone(), status },
        );
        let _prev = app.state.ext_mut::<BridgeState>().thread_statuses.insert(thread_id, status);
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
    });

    // Emit the durable roster delta so the backend view reflects the new
    // thread in ms (Leg 0 keystone) — a fresh, empty thread is the user's turn
    // (they must type the first message). Routed through `wire_turn` so the
    // emitted status matches what the status chokepoint would emit, and the
    // status memo is primed to this value so creating then immediately sending
    // a message produces exactly one follow-up `ThreadStatusChanged`.
    let created_turn = wire_turn(ThreadStatus::TheirTurn);
    emit_roster_delta(state, OpEntryKind::ThreadCreated {
        thread_id: id.clone(),
        name: name.to_owned(),
        status: created_turn,
        timestamp_ms: now_ms(),
    });
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
        state.flags.ui.dirty = true;
        log::info!("bridge: restored thread {thread_id}");
    } else {
        log::warn!("bridge: RestoreThread for unknown thread {thread_id}");
    }
}

// ── Stop / Interrupt ────────────────────────────────────────────────────

/// Stop the current stream (mirrors the Esc-key `StopStreaming` action).
fn apply_stop(state: &mut State) {
    use cp_base::state::flags::StreamPhase;

    if state.flags.stream.phase.is_streaming() {
        state.flags.stream.phase.transition(StreamPhase::Idle);
        if let Some(ctx) = state
            .context
            .iter_mut()
            .find(|c| c.context_type.as_str() == cp_base::state::context::Kind::CONVERSATION)
        {
            ctx.token_count = ctx.token_count.saturating_sub(state.streaming_estimated_tokens);
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
