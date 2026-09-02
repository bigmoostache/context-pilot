use serde::{Deserialize, Serialize};

use cp_base::state::runtime::State;

/// A scratchpad cell for storing temporary notes/data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScratchpadCell {
    /// Cell ID (C1, C2, ...)
    pub id: String,
    /// Owning thread id (compulsory, thread-owned scratchpad rework — mirrors
    /// `TodoItem.thread_id`). A cell with no owning thread cannot exist; legacy
    /// cells lacking one are purged on load. Serialized unconditionally (no
    /// skip) — it is a foreign key.
    #[serde(default)]
    pub thread_id: String,
    /// Cell title
    pub title: String,
    /// Cell content
    pub content: String,
}

/// Module-owned state for the Scratchpad module
#[derive(Debug)]
pub struct ScratchpadState {
    /// All scratchpad cells (across all threads), ordered by creation.
    pub scratchpad_cells: Vec<ScratchpadCell>,
    /// Counter for generating unique IDs (C1, C2, ...).
    pub next_scratchpad_id: usize,
    /// Injected focused-thread id for panel + tool scoping (thread-owned
    /// scratchpad rework, mirrors `TodoState.focus_filter`). The main crate
    /// stamps the current focused thread id here on focus change; the panel
    /// renders only cells whose `thread_id` matches, and the tools attach /
    /// edit / wipe cells within that thread. **Transient** — never serialized.
    pub focus_filter: Option<String>,
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
        Self { scratchpad_cells: vec![], next_scratchpad_id: 1, focus_filter: None }
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
