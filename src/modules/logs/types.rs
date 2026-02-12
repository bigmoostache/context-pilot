use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: String,
    pub timestamp_ms: u64,
    pub content: String,
}

impl LogEntry {
    pub fn new(id: String, content: String) -> Self {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Self {
            id,
            timestamp_ms,
            content,
        }
    }

    /// Create a log entry with an explicit timestamp (ms since UNIX epoch).
    pub fn with_timestamp(id: String, content: String, timestamp_ms: u64) -> Self {
        Self {
            id,
            timestamp_ms,
            content,
        }
    }
}
