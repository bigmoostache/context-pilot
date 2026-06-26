//! Live thread-archived emission for the main event loop.
//!
//! The `emit_thread_archived` function is a main-loop **observe-on-change
//! chokepoint**: it diffs each thread's live `archived` flag against the
//! per-thread snapshot held in [`BridgeState::thread_archived_memo`] and
//! emits a durable [`ThreadArchived`] / [`ThreadRestored`] delta only on an
//! actual change — capturing mutations from *every* source (web command, TUI
//! action, the `Send` tool's auto-unarchive) with one uniform path.

use cp_base::state::runtime::State;
use cp_mod_bridge::BridgeState;
use cp_mod_threads::types::ThreadsState;
use cp_wire::types::oplog::OpEntryKind;

use super::bridge::{bridge_active, emit_roster_delta};
use crate::app::App;

/// Replay the agent's oplog to recover the **last per-thread archived flag
/// the log recorded** — i.e. exactly what the backend's view has folded.
///
/// This is the correct seed for the archived chokepoint on the first pass
/// after a (re)boot: comparing live `thread.archived` against this (not
/// against the live flag itself) ensures that any flip that landed on disk
/// while the bridge was down but was never journaled is emitted on the very
/// first pass — self-healing disk↔oplog divergence.
///
/// Returns an empty map when the bridge is OFF or replay fails (degraded:
/// every live thread looks "new" and has its current archived flag emitted).
fn oplog_roster_archived(state: &State) -> std::collections::HashMap<String, bool> {
    let Some(bs) = state.get_ext::<BridgeState>() else {
        return std::collections::HashMap::new();
    };
    let Some(boot) = bs.boot.as_ref() else {
        return std::collections::HashMap::new();
    };
    match cp_oplog::replay::replay(&boot.entry().oplog_path) {
        Ok(recovered) => recovered.roster.into_iter().map(|t| (t.thread_id, t.archived)).collect(),
        Err(e) => {
            log::warn!("bridge: oplog replay for archived seed failed: {e:?}");
            std::collections::HashMap::new()
        }
    }
}

/// Emit [`ThreadArchived`] / [`ThreadRestored`] the instant any thread's
/// archived flag flips, so the backend view (and the web roster) hide or
/// reveal the thread in milliseconds.
///
/// Mirrors [`emit_thread_status`](super::bridge::emit_thread_status) exactly:
/// seeds from the oplog roster on the first pass (what the view has folded),
/// then falls through to the diff so any flip the oplog missed while the
/// bridge was down is emitted immediately. Rides the **durable** path
/// ([`emit_roster_delta`]) — an archived/restored transition is user-visible
/// roster state that must never be silently lost.
///
/// No-op when the bridge is OFF.
pub(in crate::app::run) fn emit_thread_archived(app: &mut App) {
    if !bridge_active(&app.state) {
        return;
    }

    // Emit the changed count only once (when the diff fires on the first tick).
    // Normal ticks have zero changes — no log noise.

    // First pass after (re)boot: seed from the oplog, then FALL THROUGH.
    let seeded = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.seeded.archived());
    if !seeded {
        let oplog_archived = oplog_roster_archived(&app.state);
        log::info!("bridge: emit_thread_archived seeding from oplog roster ({} entries)", oplog_archived.len(),);
        let bs = app.state.ext_mut::<BridgeState>();
        bs.thread_archived_memo.extend(oplog_archived);
        bs.seeded.seed_archived();
        // Fall through — diff below emits any flip the oplog missed.
    }

    // Diff live archived flags against the memo; collect changes (owned).
    let changed: Vec<(String, bool)> = {
        let ts = ThreadsState::get(&app.state);
        let memo = &app.state.ext::<BridgeState>().thread_archived_memo;
        ts.threads
            .iter()
            .filter_map(|t| {
                let live = t.archived;
                (memo.get(&t.id).copied() != Some(live)).then(|| (t.id.clone(), live))
            })
            .collect()
    };

    if !changed.is_empty() {
        log::info!("bridge: emit_thread_archived found {} divergence(s)", changed.len(),);
    }

    for (thread_id, archived) in changed {
        log::info!("bridge: emit_thread_archived divergence: {thread_id} → archived={archived}",);
        let kind = if archived {
            OpEntryKind::ThreadArchived { thread_id: thread_id.clone() }
        } else {
            OpEntryKind::ThreadRestored { thread_id: thread_id.clone() }
        };
        emit_roster_delta(&app.state, kind);
        let _prev = app.state.ext_mut::<BridgeState>().thread_archived_memo.insert(thread_id, archived);
    }
}
