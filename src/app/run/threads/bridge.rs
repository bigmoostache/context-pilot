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
use cp_mod_threads::types::{FocusState, ThreadStatus, ThreadsState};
use cp_wire::types::command::Command;
use cp_wire::types::oplog::OpEntryKind;
use cp_wire::types::{Phase, ThreadTurn};

use crate::app::App;

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
///
/// When at least one command applied, the agent's tier-② state is persisted
/// (`save_state_async`). This is load-bearing for commands whose effect lives
/// **only** in config.json and has no oplog-delta representation — chiefly
/// `Configure` (LLM provider + model): without a save, the change exists only
/// in memory and is lost on the next reload, which is read back from
/// config.json via `/meta` (the "provider reverts after Ctrl+R" bug). Roster
/// mutations already ride the oplog→view for live reads, so the save is a
/// promptness bonus for their disk backstop, never a correctness dependency.
/// The write is async + click-frequency, so it never burdens the loop.
pub(in crate::app::run) fn poll_bridge_commands(app: &mut App) {
    let mut budget = DRAIN_BUDGET;
    let mut applied_any = false;
    while budget > 0 {
        budget = budget.saturating_sub(1);
        let Some(commands) = accept_commands(&mut app.state) else {
            break; // no pending connection — done draining this tick.
        };
        for cmd in commands {
            super::commands::apply_command(app, cmd);
            applied_any = true;
        }
    }
    if applied_any {
        app.save_state_async();
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
    let (Some(boot), Some(intake)) = (bs.boot.as_ref(), bs.intake.as_mut()) else {
        return None;
    };

    // Non-blocking accept — WouldBlock means no connection is pending.
    let Ok((mut stream, _addr)) = boot.listener().accept() else {
        return None;
    };

    // The accepted stream INHERITS the listener's non-blocking flag on macOS
    // (and Linux without `accept4`). Left non-blocking, `handle_connection`'s
    // read loop hits a transient `WouldBlock` the instant the socket buffer
    // momentarily drains mid-frame and treats it as fatal — so any command
    // larger than the kernel's AF_UNIX socket buffer (~8 KiB) closes the
    // connection before it is fully read, surfacing to the backend as a
    // `BrokenPipe`/`502 agent unreachable` (the "big messages don't go through"
    // bug, T274). Forcing the stream back to BLOCKING makes the per-read
    // timeout below the real wedge-guard: each read blocks (up to the timeout)
    // for the next chunk instead of erroring on a normal in-flight gap, so a
    // multi-MiB frame streams in correctly.
    let _set_blocking = stream.set_nonblocking(false);

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
pub(super) fn emit_roster_delta(state: &State, kind: OpEntryKind) {
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
    if matches!(phase, StreamPhase::Receiving) {
        Phase::Streaming
    } else if matches!(phase, StreamPhase::ExecutingTools) {
        Phase::Tooling
    } else {
        Phase::Idle
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
    let phase_changed = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.last_phase != Some(phase));
    if phase_changed {
        emit_best_effort(&app.state, OpEntryKind::PhaseTransition { phase });
        app.state.ext_mut::<BridgeState>().last_phase = Some(phase);
    }

    // Cost — cumulative-since-boot; emit when the dollar total moves.
    let cost_usd =
        cp_base::cast::float_math::sum3(app.state.cost_hit_usd, app.state.cost_miss_usd, app.state.cost_output_usd);
    let cost_changed = app
        .state
        .get_ext::<BridgeState>()
        .is_some_and(|bs| cp_base::cast::float_math::abs_diff(bs.last_cost_usd, cost_usd) > f64::EPSILON);
    if cost_changed {
        let input_tokens = app.state.cache_hit_tokens.to_u64().saturating_add(app.state.cache_miss_tokens.to_u64());
        let output_tokens = app.state.total_output_tokens.to_u64();
        emit_best_effort(&app.state, OpEntryKind::CostAggregate { input_tokens, output_tokens, cost_usd });
        app.state.ext_mut::<BridgeState>().last_cost_usd = cost_usd;
    }

    // Context-window occupancy — the agent's authoritative `used/threshold/
    // budget` triple PLUS its cache `hit/miss` split (the SAME canonical
    // helpers the TUI sidebar token bar renders), so the web HUD meter and its
    // `Used (hit)` / `Used (miss)` breakdown are byte-identical to ratatui
    // (T297). Emit on change — the memo carries hit too, so a hit↔miss flip at
    // an unchanged total still re-emits.
    let (used, threshold, budget) = crate::modules::overview::context::context_usage(&app.state);
    let (hit, miss) = crate::modules::overview::context::context_hit_miss(&app.state);
    let ctx_tuple = (used.to_u64(), threshold.to_u64(), budget.to_u64(), hit.to_u64(), miss.to_u64());
    let ctx_changed = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.last_context != Some(ctx_tuple));
    if ctx_changed {
        emit_best_effort(
            &app.state,
            OpEntryKind::ContextUsage {
                used_tokens: ctx_tuple.0,
                threshold_tokens: ctx_tuple.1,
                budget_tokens: ctx_tuple.2,
                hit_tokens: ctx_tuple.3,
                miss_tokens: ctx_tuple.4,
            },
        );
        app.state.ext_mut::<BridgeState>().last_context = Some(ctx_tuple);
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
pub(super) const fn wire_turn(status: ThreadStatus) -> ThreadTurn {
    if matches!(status, ThreadStatus::MyTurn) { ThreadTurn::MyTurn } else { ThreadTurn::TheirTurn }
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
/// Replay the agent's own oplog to recover the **last per-thread status the log
/// recorded** — i.e. exactly what the backend's view has folded.
///
/// This is the correct seed for [`emit_thread_status`]'s change-memo on the
/// first pass after a (re)boot: comparing the live (disk) status against *this*
/// (not against the live status itself) makes the first diff emit precisely the
/// transitions that landed on disk while the bridge was down but were never
/// journaled — self-healing disk↔oplog divergence (see the seed comment in
/// [`emit_thread_status`]).
///
/// Returns an empty map when the bridge is OFF or the replay fails: the caller
/// then seeds nothing, so every live thread looks "new" to the diff and has its
/// current status emitted — a safe, if chattier, degradation that still
/// converges the view (it never leaves a thread stale).
fn oplog_roster_statuses(state: &State) -> std::collections::HashMap<String, ThreadTurn> {
    let Some(bs) = state.get_ext::<BridgeState>() else {
        return std::collections::HashMap::new();
    };
    let Some(boot) = bs.boot.as_ref() else {
        return std::collections::HashMap::new();
    };
    match cp_oplog::replay::replay(&boot.entry().oplog_path) {
        Ok(recovered) => recovered.roster.into_iter().map(|t| (t.thread_id, t.status)).collect(),
        Err(e) => {
            log::warn!("bridge: oplog replay for status seed failed: {e:?}");
            std::collections::HashMap::new()
        }
    }
}

/// Emit a [`ThreadStatusChanged`](OpEntryKind::ThreadStatusChanged) the instant
/// any thread's turn-status flips, so the backend view (and the web roster)
/// move the thread to the right bucket in milliseconds instead of waiting on
/// the debounced tier-② disk write.
///
/// A main-loop **observe-on-change chokepoint**: it diffs each thread's live
/// status against the per-thread snapshot held in [`BridgeState`] and emits
/// **only on an actual flip**, so it captures a transition from *every* source
/// — a web `SendMessage`, the agent's `Send` tool, a TUI reply, the agent
/// finishing a turn — with one uniform path. A status flip is user-visible
/// roster state, so it rides the **durable** (never-dropped, never-loop-
/// blocking) path ([`emit_roster_delta`]).
///
/// The first pass after a (re)boot seeds the snapshot from the **oplog roster**
/// (what the backend view has folded) and then *falls through* to the diff, so
/// any flip that landed on disk while the bridge was down but was never
/// journaled is emitted on the very first pass — self-healing disk↔oplog
/// divergence (see the inline seed comment).
///
/// No-op when the bridge is OFF.
pub(in crate::app::run) fn emit_thread_status(app: &mut App) {
    if !bridge_active(&app.state) {
        return;
    }

    // First pass after (re)boot: seed the memo from the **oplog's** last-known
    // per-thread status — i.e. exactly what the backend's view has folded — and
    // then FALL THROUGH to the diff+emit below.
    //
    // The memo must be seeded from the oplog, NOT from the live (disk) status:
    // a turn flip that lands on disk while the bridge is down (e.g. a `Send`
    // immediately before a `system_reload`, or any restart straddling a
    // transition) is saved to tier-② config.json by the normal save path but
    // never journaled as a `ThreadStatusChanged` delta. If we seeded from the
    // live status, that post-flip value would become the baseline and the lost
    // transition would be swallowed forever — leaving the backend view (rebuilt
    // from the oplog) permanently stale (the "thread shows under the wrong
    // turn" bug). Seeding from the oplog roster and then running the normal
    // diff makes the very first pass emit precisely the transitions the oplog
    // is missing, self-healing disk↔oplog divergence on every boot. Threads the
    // oplog has never recorded a status for are simply absent from the seed, so
    // the diff emits their current status (correct — the view doesn't know it).
    let seeded = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.seeded.statuses());
    if !seeded {
        let oplog_statuses = oplog_roster_statuses(&app.state);
        let bs = app.state.ext_mut::<BridgeState>();
        // `extend` (not an explicit `for` over the map) keeps us off the
        // `iter_over_hash_type` lint; insertion order is irrelevant since each
        // thread id appears once in the roster and the memo is keyed by id.
        bs.thread_statuses.extend(oplog_statuses);
        bs.seeded.seed_statuses();
        // Intentionally NO early return: fall through so the diff below emits
        // any flip the oplog missed while the bridge was down.
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
        emit_roster_delta(&app.state, OpEntryKind::ThreadStatusChanged { thread_id: thread_id.clone(), status });
        let _prev = app.state.ext_mut::<BridgeState>().thread_statuses.insert(thread_id, status);
    }
}

// ── Thread focus emission (focused-thread highlight — design doc I8) ──────

/// Emit a [`ThreadFocusChanged`](OpEntryKind::ThreadFocusChanged) the instant
/// the agent's focused thread changes, so the backend view (and the web UI's
/// focused-thread highlight) reflect it in milliseconds instead of waiting on
/// the debounced tier-② disk write plus the frontend's backstop poll.
///
/// Like [`emit_thread_status`] this is a main-loop **observe-on-change
/// chokepoint**: it diffs the live [`FocusState::focused_thread_id`] against the
/// snapshot held in [`BridgeState::last_focus`] and emits **only on an actual
/// change**, so it captures focus from *every* source with one uniform path —
/// the idle `MY_TURN` auto-`Read` ([`maybe_inject_auto_read`](super::maybe_inject_auto_read)),
/// a manual `Read`, or focus release on archive / a finished turn — rather than
/// an emit call scattered at each focus-mutation site.
///
/// Focus is ephemeral, disposable UI state (the same class as phase), so it
/// rides the **best-effort** path ([`emit_best_effort`]): a dropped focus delta
/// self-heals from the agent's tier-② `FocusState` on the next `/threads` read
/// and is superseded by the next focus change.
///
/// The first pass after boot **seeds** the snapshot without emitting, so a
/// (re)started agent does not replay its current focus as a spurious change
/// (the cold focus rides the frontend's initial tier-② load).
///
/// No-op when the bridge is OFF.
pub(in crate::app::run) fn emit_thread_focus(app: &mut App) {
    if !bridge_active(&app.state) {
        return;
    }

    let focused = FocusState::get(&app.state).focused_thread_id.clone();

    // First pass: snapshot the existing focus without emitting.
    let seeded = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.seeded.focus());
    if !seeded {
        let bs = app.state.ext_mut::<BridgeState>();
        bs.last_focus = focused;
        bs.seeded.seed_focus();
        return;
    }

    // Emit only on an actual change.
    let changed = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.last_focus != focused);
    if changed {
        emit_best_effort(&app.state, OpEntryKind::ThreadFocusChanged { thread_id: focused.clone() });
        app.state.ext_mut::<BridgeState>().last_focus = focused;
    }
}

// ── Behaviour emission (active behaviour-agent — design doc I8, T581) ─────

/// Emit a [`BehaviourChanged`](OpEntryKind::BehaviourChanged) the instant the
/// agent's active behaviour agent (system prompt) changes, so the web footer's
/// behaviour chip reflects it in milliseconds instead of waiting on the coarse
/// `config.json` mtime backstop (~2s) plus the invalidate throttle.
///
/// A main-loop **observe-on-change chokepoint** — the exact idiom of
/// [`emit_thread_status`] / [`emit_thread_focus`]: it diffs the live
/// [`PromptState::active_agent_id`] against the snapshot held in
/// [`BridgeState::last_behaviour`] and emits **only on an actual change**, so it
/// captures a switch from *every* source with one uniform path — the local
/// `agent_load` tool **and** a web `LoadBehaviour` command — rather than an emit
/// call scattered at each mutation site.
///
/// The active behaviour is disposable UI state (the same class as focus/phase),
/// so it rides the **best-effort** path ([`emit_best_effort`]): a dropped delta
/// self-heals via the mtime backstop and is superseded by the next change. The
/// observer (the web bridge) does not fold it — it invalidates its library query
/// so the next read surfaces the fresh active agent from tier-② `config.json`.
///
/// The first pass after boot **seeds** the snapshot without emitting, so a
/// (re)started agent does not replay its current behaviour as a spurious change
/// (the cold value rides the frontend's initial library load).
///
/// No-op when the bridge is OFF.
pub(in crate::app::run) fn emit_behaviour(app: &mut App) {
    if !bridge_active(&app.state) {
        return;
    }

    let active = cp_mod_prompt::types::PromptState::get(&app.state).active_agent_id.clone();

    // First pass: snapshot the existing active behaviour without emitting.
    let seeded = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.seeded.behaviour());
    if !seeded {
        let bs = app.state.ext_mut::<BridgeState>();
        bs.last_behaviour = active;
        bs.seeded.seed_behaviour();
        return;
    }

    // Emit only on an actual change.
    let changed = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.last_behaviour != active);
    if changed {
        emit_best_effort(&app.state, OpEntryKind::BehaviourChanged { agent_id: active.clone() });
        app.state.ext_mut::<BridgeState>().last_behaviour = active;
    }
}
