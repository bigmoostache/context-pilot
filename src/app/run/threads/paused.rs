//! Live thread-paused emission for the main event loop.
//!
//! The `emit_thread_paused` function is a main-loop **observe-on-change
//! chokepoint**: it diffs each thread's live `paused` flag against the
//! per-thread snapshot held in [`BridgeState::thread_paused_memo`] and
//! emits a durable [`ThreadPaused`] / [`ThreadResumed`] delta only on an
//! actual change — capturing mutations from *every* source (web command,
//! TUI action) with one uniform path.
//!
//! Mirrors [`emit_thread_archived`](super::archived) exactly.

use cp_base::state::runtime::State;
use cp_mod_bridge::BridgeState;
use cp_mod_threads::types::ThreadsState;
use cp_wire::types::oplog::OpEntryKind;

use super::bridge::{bridge_active, emit_roster_delta};
use crate::app::App;

/// Replay the agent's oplog to recover the **last per-thread paused flag
/// the log recorded** — i.e. exactly what the backend's view has folded.
///
/// Returns an empty map when the bridge is OFF or replay fails (degraded:
/// every live thread looks "new" and has its current paused flag emitted).
fn oplog_roster_paused(state: &State) -> std::collections::HashMap<String, bool> {
    let Some(bs) = state.get_ext::<BridgeState>() else {
        return std::collections::HashMap::new();
    };
    let Some(boot) = bs.boot.as_ref() else {
        return std::collections::HashMap::new();
    };
    match cp_oplog::replay::replay(&boot.entry().oplog_path) {
        Ok(recovered) => recovered.roster.into_iter().map(|t| (t.thread_id, t.paused)).collect(),
        Err(e) => {
            log::warn!("bridge: oplog replay for paused seed failed: {e:?}");
            std::collections::HashMap::new()
        }
    }
}

/// Emit [`ThreadPaused`] / [`ThreadResumed`] the instant any thread's
/// paused flag flips, so the backend view (and the web roster) update the
/// thread's paused state in milliseconds.
///
/// No-op when the bridge is OFF.
pub(in crate::app::run) fn emit_thread_paused(app: &mut App) {
    if !bridge_active(&app.state) {
        return;
    }

    // First pass after (re)boot: seed from the oplog, then FALL THROUGH.
    let seeded = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.seeded.paused());
    if !seeded {
        let oplog_paused = oplog_roster_paused(&app.state);
        let bs = app.state.ext_mut::<BridgeState>();
        bs.thread_paused_memo.extend(oplog_paused);
        bs.seeded.seed_paused();
        // Fall through — diff below emits any flip the oplog missed.
    }

    // Diff live paused flags against the memo; collect changes (owned).
    let changed: Vec<(String, bool)> = {
        let ts = ThreadsState::get(&app.state);
        let memo = &app.state.ext::<BridgeState>().thread_paused_memo;
        ts.threads
            .iter()
            .filter_map(|t| {
                let live = t.paused;
                (memo.get(&t.id).copied() != Some(live)).then(|| (t.id.clone(), live))
            })
            .collect()
    };

    for (thread_id, paused) in changed {
        let kind = if paused {
            OpEntryKind::ThreadPaused { thread_id: thread_id.clone() }
        } else {
            OpEntryKind::ThreadResumed { thread_id: thread_id.clone() }
        };
        emit_roster_delta(&app.state, kind);
        let _prev = app.state.ext_mut::<BridgeState>().thread_paused_memo.insert(thread_id, paused);
    }
}
