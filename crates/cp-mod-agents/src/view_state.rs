//! Read-only execution-state snapshot for the agents view.
//!
//! This is a **derived mirror**, not a source of truth. `App::fleet`
//! holds the authoritative [`Entry`](cp_fleet::Entry) per non-executing agent;
//! this map is rewritten in full every tick from that registry plus the
//! executing's own [`State`], purely so the renderer can read it without
//! threading `App` through the entire render stack.
//!
//! **Never read this for scheduling.** Promotion, the `K` cap, and the step
//! loop all consult `App::fleet`. Reading a mirror for a scheduling decision
//! reintroduces the split-brain this type exists to avoid — the mirror can be
//! one tick stale and omits the executing by construction.

use std::collections::HashMap;

use cp_base::state::runtime::State;
use cp_fleet::AgentExecState;
use serde::{Deserialize, Serialize};

/// Per-thread execution state for the agents view. Registered as UI-global
/// module state and never persisted: rebuilt from the fleet registry each tick.
///
/// An agent absent from the map is not an error — archived agents have no
/// registry entry, and a freshly created agent has none until the next
/// reconcile pass. Renderers fall back to [`AgentExecState::Idle`] rather than
/// failing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FleetExecMirror {
    /// Last-known execution state per agent id.
    ///
    /// Includes the focused agent, whose state is derived from the executing
    /// [`State`] rather than the registry — the registry deliberately excludes
    /// the focused agent because its context lives flat in `State` (the
    /// executing=focused invariant).
    pub exec_states: HashMap<String, AgentExecState>,
}

impl FleetExecMirror {
    /// An empty snapshot (no agent has been observed yet).
    ///
    /// Not `const`: unlike `AgentsState::new`, this holds a `HashMap`, which
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

    /// Execution state for `agent_id`, defaulting to `Idle` when absent.
    ///
    /// Defaulting (rather than `Option`) keeps the caller's match total: a
    /// agent with no registry entry is indistinguishable from one with nothing
    /// to do, which is true for archived and just-created agents alike.
    #[must_use]
    pub fn exec_state_of(&self, agent_id: &str) -> AgentExecState {
        self.exec_states.get(agent_id).copied().unwrap_or_default()
    }

    /// Replace the whole snapshot.
    ///
    /// Rebuilding rather than merging matters: an agent deleted or archived
    /// since the last tick must *disappear* from the map, not linger with a
    /// stale state forever.
    pub fn replace_all(&mut self, exec_states: HashMap<String, AgentExecState>) {
        self.exec_states = exec_states;
    }
}
