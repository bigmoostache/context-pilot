//! Focus + behaviour observe-on-change emitters.
//!
//! Two of the bridge's main-loop chokepoints, split out of
//! [`bridge`](super::bridge) so it stays within the workspace's 500-line
//! budget. Both follow the identical idiom used by
//! [`emit_thread_status`](super::bridge::emit_thread_status): diff the live
//! value against the snapshot held in [`BridgeState`], emit **only** on an
//! actual change, and seed (without emitting) on the first pass after boot so a
//! restarted agent never replays its current state as a spurious change.
//!
//! Both ride the **best-effort** path — focus and active behaviour are
//! disposable UI state, so a dropped delta self-heals from the agent's tier-②
//! files on the next read.

use cp_mod_bridge::BridgeState;
use cp_mod_threads::types::FocusState;
use cp_wire::types::oplog::OpEntryKind;

use crate::app::App;

use super::bridge::{bridge_active, emit_best_effort};

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
