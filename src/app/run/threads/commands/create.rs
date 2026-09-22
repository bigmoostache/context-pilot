//! Thread *creation* commands — `CreateThread` and `BranchThread`.
//!
//! Both end the same way: announce the new thread on the oplog, then apply the
//! optional pause and first message in strict create -> pause -> send order
//! (T687). A branch additionally starts with a copy of its parent's history,
//! which `emit_messages` streams out on the next tick (the new thread has no
//! message-count memo yet, so every copied message becomes a `MessageCreated`).

use cp_base::state::runtime::State;
use cp_mod_bridge::BridgeState;
use cp_mod_threads::types::{ThreadStatus, ThreadsState};
use cp_wire::types::oplog::OpEntryKind;

use crate::app::panels::now_ms;

use super::super::bridge::{emit_roster_delta, wire_turn};
use super::{apply_pause_thread, apply_send_message};

/// What a freshly created thread is seeded with once it exists.
pub(super) struct Seed<'seed> {
    /// Optional first user message (blank = none).
    pub initial_message: Option<&'seed str>,
    /// Create the thread already paused.
    pub paused: bool,
}

/// The message a `BranchThread` cuts its parent at (inclusive).
pub(super) struct BranchPoint<'point> {
    /// Parent thread id.
    pub source_thread_id: &'point str,
    /// Epoch-ms timestamp of the last parent message to copy.
    pub message_ts: u64,
}

/// Create a new thread with the given name, optionally seeding a first user
/// message and starting it paused.
///
/// Strict order (T687): create, then pause (if requested), then seed the first
/// message through [`apply_send_message`]. Pausing before the send guarantees
/// the seeded message lands on an already-paused thread, so it can never nudge
/// the agent — the whole sequence is one atomic command application, with no
/// frontend id round-trip and no window where the message exists un-paused.
/// The message content rides the durable command payload, closing the
/// data-loss race a frontend create -> wait-id -> send orchestration had.
pub(super) fn apply_create_thread(state: &mut State, name: &str, seed: &Seed<'_>) {
    let ts = ThreadsState::get_mut(state);
    let id = format!("T{}", ts.next_id);
    ts.next_id = ts.next_id.saturating_add(1);

    ts.threads.push(cp_mod_threads::types::Thread::new(id.clone(), name.to_owned()));
    announce_thread(state, &id, name, None);
    log::info!("bridge: created thread {id} \"{name}\"");
    apply_seed(state, &id, seed);
}

/// Branch a new thread out of an existing one: copy the parent's messages up
/// to and including the branch point (see [`ThreadsState::branch`]), then seed
/// it exactly like [`apply_create_thread`].
///
/// The branch starts as `TheirTurn`; a first message flips it to `MyTurn`
/// through [`apply_send_message`]. An unknown parent or branch point is
/// logged and ignored — nothing is created.
pub(super) fn apply_branch_thread(state: &mut State, point: &BranchPoint<'_>, name: &str, seed: &Seed<'_>) {
    let source = point.source_thread_id;
    let id = match ThreadsState::get_mut(state).branch(source, point.message_ts, name) {
        Ok(id) => id,
        Err(e) => {
            log::warn!("bridge: BranchThread rejected: {e}");
            return;
        }
    };
    announce_thread(state, &id, name, Some(source));
    log::info!("bridge: branched thread {id} \"{name}\" out of {source} at ts={}", point.message_ts);
    apply_seed(state, &id, seed);
}

/// Emit the durable `ThreadCreated` roster delta for a thread just pushed onto
/// [`ThreadsState`], so the backend view reflects it in ms (Leg 0 keystone).
///
/// A new thread is the user's turn. The status goes through `wire_turn` so it
/// matches what the status chokepoint would emit, and the status memo is primed
/// to it so a create immediately followed by a send produces exactly one
/// follow-up `ThreadStatusChanged`.
fn announce_thread(state: &mut State, id: &str, name: &str, branched_from: Option<&str>) {
    let created_turn = wire_turn(ThreadStatus::TheirTurn);
    emit_roster_delta(
        state,
        OpEntryKind::ThreadCreated {
            thread_id: id.to_owned(),
            name: name.to_owned(),
            status: created_turn,
            timestamp_ms: now_ms(),
            branched_from: branched_from.map(str::to_owned),
        },
    );
    if let Some(bs) = state.get_ext_mut::<BridgeState>() {
        let _prev = bs.thread_statuses.insert(id.to_owned(), created_turn);
    }
    state.flags.ui.dirty = true;
}

/// Apply a [`Seed`] to the just-created thread `id` in strict pause -> send
/// order (T687): pausing FIRST means the seeded message lands on an
/// already-paused thread and can never nudge the agent, even transiently.
fn apply_seed(state: &mut State, id: &str, seed: &Seed<'_>) {
    if seed.paused {
        apply_pause_thread(state, id);
    }
    if let Some(content) = seed.initial_message.filter(|c| !c.trim().is_empty()) {
        apply_send_message(state, id, content);
    }
}
