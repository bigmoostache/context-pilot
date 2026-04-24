use cp_base::cast::Safe as _;
use cp_base::state::runtime::State;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// A timestamped log entry, optionally nested under a summary parent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Log ID (L1, L2, ...).
    pub id: String,
    /// Timestamp (ms since UNIX epoch) when the entry was created.
    pub timestamp_ms: u64,
    /// Short, atomic log text.
    pub content: String,
    /// If this log was summarized into a parent, the parent's ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// IDs of children logs that this entry summarizes (empty for leaf logs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children_ids: Vec<String>,
}

impl LogEntry {
    /// Create a log entry timestamped to now.
    #[must_use]
    pub fn new(id: String, content: String) -> Self {
        let timestamp_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().to_u64());
        Self { id, timestamp_ms, content, parent_id: None, children_ids: vec![] }
    }

    /// Create a log entry with an explicit timestamp (ms since UNIX epoch).
    #[must_use]
    pub const fn with_timestamp(id: String, content: String, timestamp_ms: u64) -> Self {
        Self { id, timestamp_ms, content, parent_id: None, children_ids: vec![] }
    }

    /// Whether this log is a summary (has children).
    #[must_use]
    pub const fn is_summary(&self) -> bool {
        !self.children_ids.is_empty()
    }

    /// Whether this log is top-level (no parent).
    #[must_use]
    pub const fn is_top_level(&self) -> bool {
        self.parent_id.is_none()
    }
}

/// Module-owned state for the Logs module
#[derive(Debug)]
pub struct LogsState {
    /// All log entries (top-level + children), ordered by creation.
    pub logs: Vec<LogEntry>,
    /// Counter for generating unique IDs (L1, L2, ...).
    pub next_log_id: usize,
    /// IDs of summary logs currently expanded (showing children).
    pub open_log_ids: Vec<String>,
}

impl Default for LogsState {
    fn default() -> Self {
        Self::new()
    }
}

impl LogsState {
    /// Create an empty state with ID counter at 1.
    #[must_use]
    pub const fn new() -> Self {
        Self { logs: vec![], next_log_id: 1, open_log_ids: vec![] }
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
