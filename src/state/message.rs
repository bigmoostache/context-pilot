use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    TextMessage,
    ToolCall,
    ToolResult,
}

impl Default for MessageType {
    fn default() -> Self {
        Self::TextMessage
    }
}

/// Message status for context management
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    #[default]
    Full,
    Summarized,
    Deleted,
    Detached,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseRecord {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultRecord {
    pub tool_use_id: String,
    pub content: String,
    #[serde(default)]
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Display ID (e.g., U1, A1, T1 - for UI/LLM)
    pub id: String,
    /// Internal UID (e.g., UID_42_U - never shown to UI/LLM)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    pub role: String,
    #[serde(default)]
    pub message_type: MessageType,
    pub content: String,
    #[serde(default)]
    pub content_token_count: usize,
    #[serde(default)]
    pub tl_dr: Option<String>,
    #[serde(default)]
    pub tl_dr_token_count: usize,
    /// Message status for context management
    #[serde(default)]
    pub status: MessageStatus,
    /// Tool uses in this message (for assistant messages)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_uses: Vec<ToolUseRecord>,
    /// Tool results in this message (for ToolResult messages)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_results: Vec<ToolResultRecord>,
    /// Input tokens used for this response (from API, for assistant messages)
    #[serde(default)]
    pub input_tokens: usize,
    /// Timestamp when this message was created (ms since UNIX epoch)
    #[serde(default)]
    pub timestamp_ms: u64,
}
