//! Live thread-paused emission for the main event loop.
//!
//! The `emit_agent_paused` function is a main-loop **observe-on-change
//! chokepoint**: it diffs each thread's live `paused` flag against the
//! per-thread snapshot held in [`BridgeState::agent_paused_memo`] and
//! emits a durable [`AgentPaused`] / [`AgentResumed`] delta only on an
//! actual change — capturing mutations from *every* source (web command,
//! TUI action) with one uniform path.
//!
//! Mirrors [`emit_agent_archived`](super::archived) exactly.

use cp_base::state::runtime::State;
use cp_mod_agents::types::AgentsState;
use cp_mod_bridge::BridgeState;
use cp_wire::types::oplog::OpEntryKind;

use super::bridge::{bridge_active, emit_roster_delta};
use crate::app::App;

/// Replay the agent's oplog to recover the **last per-thread paused flag
/// the log recorded** — i.e. exactly what the backend's view has folded.
///
/// Returns an empty map when the bridge is OFF or replay fails (degraded:
/// every live thread looks "new" and has its current paused flag emitted).
fn oplog_roster_paused(state: &State) -> std::collections::HashMap<String, bool> {
    super::oplog_roster(state).map(|r| r.iter().map(|t| (t.agent_id.clone(), t.paused)).collect()).unwrap_or_default()
}

/// Emit [`AgentPaused`] / [`AgentResumed`] the instant any thread's
/// paused flag flips, so the backend view (and the web roster) update the
/// thread's paused state in milliseconds.
///
/// No-op when the bridge is OFF.
pub(in crate::app::run) fn emit_agent_paused(app: &mut App) {
    if !bridge_active(&app.state) {
        return;
    }

    // First pass after (re)boot: seed from the oplog, then FALL THROUGH.
    let seeded = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.seeded.paused());
    if !seeded {
        let oplog_paused = oplog_roster_paused(&app.state);
        let bs = app.state.ext_mut::<BridgeState>();
        bs.agent_paused_memo.extend(oplog_paused);
        bs.seeded.seed_paused();
        // Fall through — diff below emits any flip the oplog missed.
    }

    // Diff live paused flags against the memo; collect changes (owned).
    //
    // Only emit for threads that ARE in the memo — threads absent from the
    // oplog roster (pre-bridge legacy) are unknown to the view and emitting
    // `agent_resumed` for them is both pointless (the view ignores it) and
    // noisy (pollutes the oplog with 20+ spurious entries on every restart).
    // For those threads, the command handler emits the delta directly on the
    // first real pause/resume action.
    let changed: Vec<(String, bool)> = {
        let ts = AgentsState::get(&app.state);
        let memo = &app.state.ext::<BridgeState>().agent_paused_memo;
        ts.agents
            .iter()
            .filter_map(|t| {
                let live = t.paused;
                match memo.get(&t.id) {
                    Some(&m) if m != live => Some((t.id.clone(), live)),
                    _ => None,
                }
            })
            .collect()
    };

    for (agent_id, paused) in changed {
        let kind = if paused {
            OpEntryKind::AgentPaused { agent_id: agent_id.clone() }
        } else {
            OpEntryKind::AgentResumed { agent_id: agent_id.clone() }
        };
        emit_roster_delta(&app.state, kind);
        let _prev = app.state.ext_mut::<BridgeState>().agent_paused_memo.insert(agent_id, paused);
    }
}
