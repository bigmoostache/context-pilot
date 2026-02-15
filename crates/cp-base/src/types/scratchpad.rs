use serde::{Deserialize, Serialize};

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
