use serde::{Deserialize, Serialize};

/// Memory importance level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryImportance {
    Low,
    #[default]
    Medium,
    High,
    Critical,
}

impl MemoryImportance {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "low" => Some(MemoryImportance::Low),
            "medium" => Some(MemoryImportance::Medium),
            "high" => Some(MemoryImportance::High),
            "critical" => Some(MemoryImportance::Critical),
            _ => None,
        }
    }

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
