use serde::{Deserialize, Serialize};
use std::str::FromStr;

use cp_base::state::State;

/// Memory importance level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryImportance {
    /// Low priority — nice-to-have context.
    Low,
    #[default]
    /// Default importance for general knowledge.
    Medium,
    /// High priority — impacts workflow or architecture.
    High,
    /// Must-read — critical decisions or constraints.
    Critical,
}

impl FromStr for MemoryImportance {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "low" => Ok(MemoryImportance::Low),
            "medium" => Ok(MemoryImportance::Medium),
            "high" => Ok(MemoryImportance::High),
            "critical" => Ok(MemoryImportance::Critical),
            _ => Err(()),
        }
    }
}

impl MemoryImportance {
    /// String representation for serialization/display.
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryImportance::Low => "low",
            MemoryImportance::Medium => "medium",
            MemoryImportance::High => "high",
            MemoryImportance::Critical => "critical",
        }
    }
}

/// A memory item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    /// Memory ID (M1, M2, ...)
    pub id: String,
    /// Short summary (one-liner shown when memory is closed)
    /// Migrates from old `content` field via serde alias.
    #[serde(alias = "content")]
    pub tl_dr: String,
    /// Full contents (shown only when memory is open)
    #[serde(default)]
    pub contents: String,
    /// Importance level
    #[serde(default)]
    pub importance: MemoryImportance,
    /// Freeform labels for categorization
    #[serde(default)]
    pub labels: Vec<String>,
}

/// Module-owned state for the Memory module
#[derive(Debug)]
pub struct MemoryState {
    /// All memory items, ordered by creation.
    pub memories: Vec<MemoryItem>,
    /// Counter for generating unique IDs (M1, M2, ...).
    pub next_memory_id: usize,
    /// IDs of memories currently expanded (showing full `contents`).
    pub open_memory_ids: Vec<String>,
}

impl Default for MemoryState {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryState {
    /// Create an empty state with ID counter at 1.
    pub fn new() -> Self {
        Self { memories: vec![], next_memory_id: 1, open_memory_ids: vec![] }
    }
    /// Get shared ref from State's `TypeMap`.
    pub fn get(state: &State) -> &Self {
        state.get_ext::<Self>().expect("MemoryState not initialized")
    }
    /// Get mutable ref from State's `TypeMap`.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.get_ext_mut::<Self>().expect("MemoryState not initialized")
    }
}
