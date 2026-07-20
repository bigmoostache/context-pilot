use serde::{Deserialize, Serialize};

use cp_base::state::runtime::State;

/// A scratchpad cell for storing temporary notes/data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScratchpadCell {
    /// Cell ID (C1, C2, ...)
    pub id: String,
    /// Cell title
    pub title: String,
    /// Cell content
    pub content: String,
}

/// Module-owned state for the Scratchpad module
#[derive(Debug)]
pub struct ScratchpadState {
    /// All scratchpad cells, ordered by creation.
    pub scratchpad_cells: Vec<ScratchpadCell>,
    /// Counter for generating unique IDs (C1, C2, ...).
    pub next_scratchpad_id: usize,
}

impl Default for ScratchpadState {
    fn default() -> Self {
        Self::new()
    }
}

impl ScratchpadState {
    /// Create an empty scratchpad state with ID counter at 1.
    #[must_use]
    pub const fn new() -> Self {
        Self { scratchpad_cells: vec![], next_scratchpad_id: 1 }
    }
    /// Get shared ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }
    /// Get mutable ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }
}
