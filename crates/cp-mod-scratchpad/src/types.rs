use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use cp_base::state::runtime::State;

/// Largest accepted `title + content` of one cell, in bytes. Cells are
/// rendered verbatim into the owning agent's prompt every turn, so an
/// unbounded cell would silently inflate every request.
pub const MAX_CELL_BYTES: usize = 16 * 1024;

/// Most cells one agent may hold at once (same prompt-budget reason).
pub const MAX_CELLS_PER_AGENT: usize = 50;

/// A scratchpad cell for storing temporary notes/data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScratchpadCell {
    /// Cell ID (C1, C2, ...), unique within its owning agent only — always
    /// look a cell up by `(agent_id, id)`.
    pub id: String,
    /// Owning agent id (compulsory foreign key, mirrors `TodoItem.agent_id`).
    /// Legacy cells lacking one are purged on load.
    #[serde(default)]
    #[serde(alias = "thread_id")]
    pub agent_id: String,
    /// Cell title
    pub title: String,
    /// Cell content
    pub content: String,
}

/// Module-owned state for the Scratchpad module
#[derive(Debug)]
pub struct ScratchpadState {
    /// All scratchpad cells (across all agents), ordered by creation.
    pub scratchpad_cells: Vec<ScratchpadCell>,
    /// Next cell number per agent, so each agent numbers its own cells from
    /// C1 instead of leaking fleet-wide activity through a shared counter.
    pub next_ids: BTreeMap<String, usize>,
    /// Last focused agent id seen by the main crate. Only used as a
    /// focus-changed signal (see [`crate::tools::set_focus_filter`]); scoping
    /// itself always follows `State::executing_agent_id`. **Transient**.
    pub focus_filter: Option<String>,
    /// Change stamp, refreshed on every [`ScratchpadState::get_mut`]. Lets
    /// per-tick observers skip their diff while it is unchanged, so callers
    /// must only take `get_mut` when they will really change something.
    /// **Transient**.
    pub revision: u64,
}

/// Process-wide stamp source: unique across `set_ext` replacements, so a
/// fresh state never reuses a stamp an observer already cached.
static REVISION_SOURCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl Default for ScratchpadState {
    fn default() -> Self {
        Self::new()
    }
}

impl ScratchpadState {
    /// Create an empty scratchpad state.
    #[must_use]
    pub const fn new() -> Self {
        Self { scratchpad_cells: vec![], next_ids: BTreeMap::new(), focus_filter: None, revision: 0 }
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
    /// Get mutable ref from State's `TypeMap`, refreshing the change stamp.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    pub fn get_mut(state: &mut State) -> &mut Self {
        let ss = state.ext_mut::<Self>();
        ss.revision = REVISION_SOURCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ss
    }

    /// Cells owned by `agent_id`, in creation order.
    pub fn agent_cells<'st>(&'st self, agent_id: &'st str) -> impl Iterator<Item = &'st ScratchpadCell> + 'st {
        self.scratchpad_cells.iter().filter(move |c| c.agent_id == agent_id)
    }

    /// Number of cells owned by `agent_id` (per-agent sidebar badge).
    #[must_use]
    pub fn agent_cell_count(&self, agent_id: &str) -> usize {
        self.agent_cells(agent_id).count()
    }

    /// Whether `agent_id` owns a cell `id`.
    #[must_use]
    pub fn has_cell(&self, agent_id: &str, id: &str) -> bool {
        self.agent_cells(agent_id).any(|c| c.id == id)
    }

    /// Allocate the next cell id for `agent_id` (C1, C2, ...).
    pub fn alloc_id(&mut self, agent_id: &str) -> String {
        let next = self.next_ids.entry(agent_id.to_owned()).or_insert(1);
        let id = format!("C{next}");
        *next = next.saturating_add(1);
        id
    }

    /// Raise every agent's counter above its highest existing cell number, so
    /// state loaded from the old single-counter format never reissues an id.
    pub fn reseed_counters(&mut self) {
        for cell in &self.scratchpad_cells {
            let n = cell.id.strip_prefix('C').and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
            let next = self.next_ids.entry(cell.agent_id.clone()).or_insert(1);
            *next = (*next).max(n.saturating_add(1));
        }
    }
}
