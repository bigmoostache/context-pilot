//! Live thread-paused emission for the main event loop.
//!
//! The `emit_agent_paused` function is a main-loop **observe-on-change
//! chokepoint**: it diffs each agent's live `paused` flag against the
//! per-agent snapshot held in [`BridgeState::agent_paused_memo`] and
//! emits a durable [`AgentPaused`] / [`AgentResumed`] delta only on an
//! actual change — capturing mutations from *every* source (web command,
//! TUI action) with one uniform path.
//!
//! Mirrors [`emit_agent_archived`](super::archived) exactly.

use cp_base::state::runtime::State;
use cp_mod_agents::types::AgentsState;
use cp_mod_bridge::BridgeState;
use cp_wire::types::oplog::OpEntryKind;

use super::bridge::{bridge_active, emit_roster_delta, wire_turn};
use crate::app::App;

/// Replay the agent's oplog to recover the **last per-agent paused flag
/// the log recorded** — i.e. exactly what the backend's view has folded.
///
/// Returns an empty map when the bridge is OFF or replay fails (degraded:
/// every live agent looks "new" and has its current paused flag emitted).
fn oplog_roster_paused(state: &State) -> std::collections::HashMap<String, bool> {
    super::oplog_roster(state).map(|r| r.iter().map(|t| (t.agent_id.clone(), t.paused)).collect()).unwrap_or_default()
}

/// Emit [`AgentPaused`] / [`AgentResumed`] the instant any agent's
/// paused flag flips, so the backend view (and the web roster) update the
/// agent's paused state in milliseconds.
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
    // Only emit for agents that ARE in the memo — agents absent from the
    // oplog roster (pre-bridge legacy) are unknown to the view and emitting
    // `agent_resumed` for them is both pointless (the view ignores it) and
    // noisy (pollutes the oplog with 20+ spurious entries on every restart).
    // For those agents, the command handler emits the delta directly on the
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

/// Seed [`BridgeState::agent_name_memo`] from the oplog roster on the first
/// post-boot pass. The memo doubles as the set of agents the view knows.
fn seed_name_memo(app: &mut App) {
    let seeded = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.seeded.names());
    if seeded {
        return;
    }
    let names: Vec<(String, String)> = super::oplog_roster(&app.state)
        .map(|r| r.iter().map(|t| (t.agent_id.clone(), t.name.clone())).collect())
        .unwrap_or_default();
    let bs = app.state.ext_mut::<BridgeState>();
    bs.agent_name_memo.extend(names);
    bs.seeded.seed_names();
}

/// Emit `AgentCreated` for every live agent the view has never been told
/// about (absent from the name memo).
///
/// Creation paths that skip `announce_agent` (TUI new-agent input, a delta
/// lost on exit) otherwise leave the agent out of the view roster forever:
/// every later `AgentStatusChanged` folds as a no-op and the web shows it idle.
/// Runs first in the emitter table, so the status/archived/paused emitters
/// layer the live flags on top of this creation.
pub(in crate::app::run) fn emit_agent_adopted(app: &mut App) {
    if !bridge_active(&app.state) {
        return;
    }
    seed_name_memo(app);
    let unknown: Vec<String> = {
        let memo = &app.state.ext::<BridgeState>().agent_name_memo;
        AgentsState::get(&app.state).agents.iter().filter(|t| !memo.contains_key(&t.id)).map(|t| t.id.clone()).collect()
    };
    for agent_id in unknown {
        let (name, parent_agent_id) = AgentsState::get(&app.state)
            .agents
            .iter()
            .find(|t| t.id == agent_id)
            .map(|t| (t.name.clone(), t.parent_agent_id.clone()))
            .unwrap_or_default();
        let status = wire_turn(cp_mod_agents::types::turn::status(&app.state, &agent_id));
        log::info!("bridge: adopting unannounced agent {agent_id} into the view roster");
        emit_roster_delta(
            &app.state,
            OpEntryKind::AgentCreated {
                agent_id: agent_id.clone(),
                name: name.clone(),
                status,
                timestamp_ms: cp_base::panels::now_ms(),
                branched_from: None,
                parent_agent_id,
            },
        );
        let bs = app.state.ext_mut::<BridgeState>();
        let _prev_status = bs.agent_statuses.insert(agent_id.clone(), status);
        let _prev_name = bs.agent_name_memo.insert(agent_id, name);
    }
}

/// Emit `AgentRenamed` the instant any agent's display name changes — same
/// observe-on-change discipline, over [`BridgeState::agent_name_memo`]. Every
/// rename source (web command, TUI Ctrl+R, `Agent` tool) only mutates
/// `agent.name`; this is the single emitter.
///
/// An agent missing from the memo is adopted silently (defensive: the
/// adoption emitter normally registers it first).
pub(in crate::app::run) fn emit_agent_renamed(app: &mut App) {
    if !bridge_active(&app.state) {
        return;
    }
    seed_name_memo(app);
    // (id, name, emit?) for every agent whose memo entry is missing or stale.
    let changed: Vec<(String, String, bool)> = {
        let memo = &app.state.ext::<BridgeState>().agent_name_memo;
        AgentsState::get(&app.state)
            .agents
            .iter()
            .filter_map(|t| match memo.get(&t.id) {
                Some(m) if *m == t.name => None,
                known => Some((t.id.clone(), t.name.clone(), known.is_some())),
            })
            .collect()
    };
    for (agent_id, name, emit) in changed {
        if emit {
            emit_roster_delta(&app.state, OpEntryKind::AgentRenamed { agent_id: agent_id.clone(), name: name.clone() });
        }
        let _prev = app.state.ext_mut::<BridgeState>().agent_name_memo.insert(agent_id, name);
    }
}
