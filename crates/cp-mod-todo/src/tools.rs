//! Focus-scoping + legacy purge for the thread-owned todo model.
//!
//! The task-editing surface itself now lives in [`crate::yaml`] (the `Todo`
//! tool applies `{prev, new}` diffs to a virtual YAML, reconciled by id). This
//! module keeps only the two small pure ops the main crate still calls around
//! that: panel focus-scoping and the one-time legacy backlog purge.

use cp_base::state::runtime::State;

use crate::types::TodoState;

/// Drop every item lacking a `thread_id` (the legacy, pre-rework backlog).
/// Called once on load — a permanent, forever purge (FR4).
pub fn purge_threadless(state: &mut State) {
    TodoState::get_mut(state).todos.retain(|t| !t.thread_id.is_empty());
}

/// Set the injected focused-thread filter used by the panel. Returns whether it
/// changed (which drives the caller's forced panel refresh).
pub fn set_focus_filter(state: &mut State, thread_id: Option<String>) -> bool {
    let ts = TodoState::get_mut(state);
    if ts.focus_filter == thread_id {
        false
    } else {
        ts.focus_filter = thread_id;
        true
    }
}
