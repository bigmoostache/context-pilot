//! Focus + behaviour observe-on-change emitters.
//!
//! Two of the bridge's main-loop chokepoints, split out of
//! [`bridge`](super::bridge) so it stays within the workspace's 500-line
//! budget. Both follow the identical idiom used by
//! [`emit_agent_status`](super::bridge::emit_agent_status): diff the live
//! value against the snapshot held in [`BridgeState`], emit **only** on an
//! actual change, and seed (without emitting) on the first pass after boot so a
//! restarted agent never replays its current state as a spurious change.
//!
//! Both ride the **best-effort** path — focus and active behaviour are
//! disposable UI state, so a dropped delta self-heals from the agent's tier-②
//! files on the next read.

use cp_mod_agents::types::{AgentsState, FocusState};
use cp_mod_bridge::BridgeState;
use cp_wire::types::oplog::OpEntryKind;

use crate::app::App;

use super::bridge::{bridge_active, emit_best_effort, emit_roster_delta};

// ── Agent focus emission (focused-thread highlight — design doc I8) ──────

/// Emit a [`AgentFocusChanged`](OpEntryKind::AgentFocusChanged) the instant
/// the agent's focused agent changes, so the backend view (and the web UI's
/// focused-thread highlight) reflect it in milliseconds instead of waiting on
/// the debounced tier-② disk write plus the frontend's backstop poll.
///
/// Like [`emit_agent_status`] this is a main-loop **observe-on-change
/// chokepoint**: it diffs the live [`FocusState::focused_agent_id`] against the
/// snapshot held in [`BridgeState::last_focus`] and emits **only on an actual
/// change**, so it captures focus from *every* source with one uniform path —
/// the idle `MY_TURN` watcher (`IdleMyTurnDetector`),
/// a manual `Read`, or focus release on archive / a finished turn — rather than
/// an emit call scattered at each focus-mutation site.
///
/// Focus is ephemeral, disposable UI state (the same class as phase), so it
/// rides the **best-effort** path ([`emit_best_effort`]): a dropped focus delta
/// self-heals from the agent's tier-② `FocusState` on the next `/agents` read
/// and is superseded by the next focus change.
///
/// The first pass after boot **seeds** the snapshot without emitting, so a
/// (re)started agent does not replay its current focus as a spurious change
/// (the cold focus rides the frontend's initial tier-② load).
///
/// No-op when the bridge is OFF.
pub(in crate::app::run) fn emit_agent_focus(app: &mut App) {
    if !bridge_active(&app.state) {
        return;
    }

    let focused = FocusState::get(&app.state).focused_agent_id.clone();

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
        emit_best_effort(&app.state, OpEntryKind::AgentFocusChanged { agent_id: focused.clone() });
        app.state.ext_mut::<BridgeState>().last_focus = focused;
    }
}

// ── Sidebar light emission ────────────────────────────────────────────────

/// One agent's sidebar state as shipped to the web: the light, plus the ⚠
/// reason ([`agent_warning`](crate::ui::agent_warning)) when it is stalled.
type LitState = (bool, Option<String>);

/// Emit an [`AgentLitChanged`](OpEntryKind::AgentLitChanged) whenever an
/// agent's sidebar light ([`agent_lit`](crate::ui::agents_view::agent_lit))
/// flips. Not a roster emitter: the light follows exec state, which the roster
/// fingerprint does not cover. Durable, so the view's roster stays right.
///
/// No-op when the bridge is OFF.
pub(in crate::app::run) fn emit_agent_lit(app: &mut App) {
    if !bridge_active(&app.state) {
        return;
    }
    let changed: Vec<(String, LitState)> = {
        let memo = &app.state.ext::<BridgeState>().agent_lits;
        AgentsState::get(&app.state)
            .agents
            .iter()
            .filter_map(|a| {
                let now = (crate::ui::agent_lit(&app.state, a), crate::ui::agent_warning(&app.state, a));
                (memo.get(&a.id) != Some(&now)).then(|| (a.id.clone(), now))
            })
            .collect()
    };
    for (agent_id, now) in changed {
        emit_roster_delta(
            &app.state,
            OpEntryKind::AgentLitChanged { agent_id: agent_id.clone(), lit: now.0, warning: now.1.clone() },
        );
        let _prev = app.state.ext_mut::<BridgeState>().agent_lits.insert(agent_id, now);
    }
}

// ── Behaviour emission (active behaviour-agent — design doc I8, T581) ─────

/// Emit a [`BehaviourChanged`](OpEntryKind::BehaviourChanged) the instant the
/// agent's active behaviour agent (system prompt) changes, so the web footer's
/// behaviour chip reflects it in milliseconds instead of waiting on the coarse
/// `config.json` mtime backstop (~2s) plus the invalidate throttle.
///
/// A main-loop **observe-on-change chokepoint** — the exact idiom of
/// [`emit_agent_status`] / [`emit_agent_focus`]: it diffs the live
/// [`PromptState::active_system_id`] against the snapshot held in
/// [`BridgeState::last_behaviour`] and emits **only on an actual change**, so it
/// captures a switch from *every* source with one uniform path — the local
/// `system_load` tool **and** a web `LoadBehaviour` command — rather than an emit
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

    let active = cp_mod_prompt::types::PromptState::get(&app.state).active_system_id.clone();

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
        emit_best_effort(&app.state, OpEntryKind::BehaviourChanged { system_id: active.clone() });
        app.state.ext_mut::<BridgeState>().last_behaviour = active;
    }
}
