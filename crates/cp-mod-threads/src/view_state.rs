//! Read-only execution-state snapshot for the threads view.
//!
//! This is a **derived mirror**, not a source of truth. `App::fleet`
//! holds the authoritative [`Entry`](cp_fleet::Entry) per non-resident thread;
//! this map is rewritten in full every tick from that registry plus the
//! resident's own [`State`], purely so the renderer can read it without
//! threading `App` through the entire render stack.
//!
//! **Never read this for scheduling.** Promotion, the `K` cap, and the step
//! loop all consult `App::fleet`. Reading a mirror for a scheduling decision
//! reintroduces the split-brain this type exists to avoid — the mirror can be
//! one tick stale and omits the resident by construction.

use std::collections::HashMap;

use cp_base::state::runtime::State;
use cp_fleet::ThreadExecState;
use serde::{Deserialize, Serialize};

/// Per-thread execution state for the threads view. Registered as UI-global
/// module state and never persisted: rebuilt from the fleet registry each tick.
///
/// A thread absent from the map is not an error — archived threads have no
/// registry entry, and a freshly created thread has none until the next
/// reconcile pass. Renderers fall back to [`ThreadExecState::Idle`] rather than
/// failing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FleetExecMirror {
    /// Last-known execution state per thread id.
    ///
    /// Includes the focused thread, whose state is derived from the resident
    /// [`State`] rather than the registry — the registry deliberately excludes
    /// the focused thread because its context lives flat in `State` (the
    /// resident=focused invariant).
    pub exec_states: HashMap<String, ThreadExecState>,
}

impl FleetExecMirror {
    /// An empty snapshot (no thread has been observed yet).
    ///
    /// Not `const`: unlike `ThreadsState::new`, this holds a `HashMap`, which
    /// has no const constructor.
    #[must_use]
    pub fn new() -> Self {
        Self { exec_states: HashMap::new() }
    }

    /// Get shared ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if `FleetExecMirror` was never inserted into state.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Get mutable ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if `FleetExecMirror` was never inserted into state.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }

    /// Execution state for `thread_id`, defaulting to `Idle` when absent.
    ///
    /// Defaulting (rather than `Option`) keeps the caller's match total: a
    /// thread with no registry entry is indistinguishable from one with nothing
    /// to do, which is true for archived and just-created threads alike.
    #[must_use]
    pub fn exec_state_of(&self, thread_id: &str) -> ThreadExecState {
        self.exec_states.get(thread_id).copied().unwrap_or_default()
    }

    /// Replace the whole snapshot.
    ///
    /// Rebuilding rather than merging matters: a thread deleted or archived
    /// since the last tick must *disappear* from the map, not linger with a
    /// stale state forever.
    pub fn replace_all(&mut self, exec_states: HashMap<String, ThreadExecState>) {
        self.exec_states = exec_states;
    }
}
