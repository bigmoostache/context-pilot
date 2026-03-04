use cp_base::cast::SafeCast;
use cp_base::state::State;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: String,
    pub timestamp_ms: u64,
    pub content: String,
    /// If this log was summarized into a parent, the parent's ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// IDs of children logs that this entry summarizes (empty for leaf logs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children_ids: Vec<String>,
}

impl LogEntry {
    pub fn new(id: String, content: String) -> Self {
        let timestamp_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis().to_u64()).unwrap_or(0);
        Self { id, timestamp_ms, content, parent_id: None, children_ids: vec![] }
    }

    /// Create a log entry with an explicit timestamp (ms since UNIX epoch).
    pub fn with_timestamp(id: String, content: String, timestamp_ms: u64) -> Self {
        Self { id, timestamp_ms, content, parent_id: None, children_ids: vec![] }
    }

    /// Whether this log is a summary (has children).
    pub fn is_summary(&self) -> bool {
        !self.children_ids.is_empty()
    }

    /// Whether this log is top-level (no parent).
    pub fn is_top_level(&self) -> bool {
        self.parent_id.is_none()
    }
}

/// Module-owned state for the Logs module
#[derive(Debug)]
pub struct LogsState {
    pub logs: Vec<LogEntry>,
    pub next_log_id: usize,
    pub open_log_ids: Vec<String>,
}

impl Default for LogsState {
    fn default() -> Self {
        Self::new()
    }
}

impl LogsState {
    pub fn new() -> Self {
        Self { logs: vec![], next_log_id: 1, open_log_ids: vec![] }
    }

    pub fn get(state: &State) -> &Self {
        state.get_ext::<Self>().expect("LogsState not initialized")
    }

    pub fn get_mut(state: &mut State) -> &mut Self {
        state.get_ext_mut::<Self>().expect("LogsState not initialized")
    }
}
