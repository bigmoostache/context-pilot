use cp_base::cast::Safe as _;
use cp_base::state::runtime::State;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// A timestamped log entry with importance and freeform tags.
///
/// Stored as chunked JSON in `.context-pilot/logs/`.  Indexed into
/// Meilisearch by the search module's file watcher for full-text search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Log ID (L1, L2, ...).
    pub id: String,
    /// Timestamp (ms since UNIX epoch) when the entry was created.
    pub timestamp_ms: u64,
    /// ISO 8601 datetime string for display (e.g. `"2026-05-06T12:00:00Z"`).
    #[serde(default)]
    pub datetime: String,
    /// Short, atomic log text.
    pub content: String,
    /// Importance level: `"low"`, `"medium"`, `"high"`, or `"critical"`.
    #[serde(default = "default_importance")]
    pub importance: String,
}

/// Default importance level for deserialization of legacy logs.
fn default_importance() -> String {
    "medium".to_string()
}

/// Format a millisecond timestamp as an ISO 8601 UTC datetime string.
fn ms_to_iso(timestamp_ms: u64) -> String {
    i64::try_from(timestamp_ms).ok().and_then(cp_mod_utilities::time::epoch_ms_to_rfc3339).unwrap_or_default()
}

impl LogEntry {
    /// Create a log entry timestamped to now.
    #[must_use]
    pub fn new(id: String, content: String) -> Self {
        let timestamp_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().to_u64());
        let datetime = ms_to_iso(timestamp_ms);
        Self { id, timestamp_ms, datetime, content, importance: "medium".to_string() }
    }

    /// Create a log entry with an explicit timestamp (ms since UNIX epoch).
    #[must_use]
    pub fn with_timestamp(id: String, content: String, timestamp_ms: u64) -> Self {
        let datetime = ms_to_iso(timestamp_ms);
        Self { id, timestamp_ms, datetime, content, importance: "medium".to_string() }
    }
}

/// Module-owned state for the Logs module
#[derive(Debug)]
pub struct LogsState {
    /// All log entries, ordered by creation.
    pub logs: Vec<LogEntry>,
    /// Counter for generating unique IDs (L1, L2, ...).
    pub next_log_id: usize,
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
        Self { logs: vec![], next_log_id: 1 }
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
