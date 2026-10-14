//! Management state: the sub-agent depth cap and the main-loop outboxes.

use cp_base::state::runtime::State;

/// Fleet-shared management state.
#[derive(Debug)]
pub struct ManagementState {
    /// Deepest allowed sub-agent level (root = 0). Persisted in
    /// `config.json` under `modules.management.max_depth`.
    pub max_depth: usize,
    /// Agents created by the tool, not yet announced on the bridge.
    pub created: Vec<String>,
    /// Agents to hard-delete, deepest first, not yet torn down.
    pub doomed: Vec<String>,
}

impl Default for ManagementState {
    fn default() -> Self {
        Self::new()
    }
}

impl ManagementState {
    /// Default cap ([`cp_mod_agents::types::tree::DEFAULT_MAX_DEPTH`]), empty outboxes.
    #[must_use]
    pub const fn new() -> Self {
        Self { max_depth: cp_mod_agents::types::tree::DEFAULT_MAX_DEPTH, created: Vec::new(), doomed: Vec::new() }
    }

    /// Shared ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if `ManagementState` was never inserted into state.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Mutable ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if `ManagementState` was never inserted into state.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }

    /// Take both outboxes: `(created, doomed)`.
    pub fn drain(state: &mut State) -> (Vec<String>, Vec<String>) {
        let ms = Self::get_mut(state);
        (std::mem::take(&mut ms.created), std::mem::take(&mut ms.doomed))
    }
}
