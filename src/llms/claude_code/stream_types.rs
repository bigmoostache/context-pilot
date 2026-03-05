//! SSE stream event deserialization types for the Claude Code API.

use serde::Deserialize;

/// Content block metadata from SSE stream events.
#[derive(Debug, Deserialize)]
pub(super) struct StreamContentBlock {
    /// Block type (e.g. `text`, `tool_use`)
    #[serde(rename = "type")]
    pub block_type: Option<String>,
    /// Block ID (for `tool_use` blocks)
    pub id: Option<String>,
    /// Tool name (for `tool_use` blocks)
    pub name: Option<String>,
}

/// Delta payload from SSE stream events.
#[derive(Debug, Deserialize)]
pub(super) struct StreamDelta {
    /// Delta type (e.g. `text_delta`, `input_json_delta`)
    #[serde(rename = "type")]
    pub delta_type: Option<String>,
    /// Text content delta
    pub text: Option<String>,
    /// Partial JSON for tool input
    pub partial_json: Option<String>,
    /// Stop reason (e.g. `end_turn`, `tool_use`)
    pub stop_reason: Option<String>,
}

/// Message body from `message_start` events.
#[derive(Debug, Deserialize)]
pub(super) struct StreamMessageBody {
    /// Token usage statistics
    pub usage: Option<StreamUsage>,
}

/// Top-level SSE stream event from the Claude Code API.
#[derive(Debug, Deserialize)]
pub(super) struct StreamMessage {
    /// Event type (e.g. `content_block_start`, `message_delta`)
    #[serde(rename = "type")]
    pub event_type: String,
    /// Content block metadata (for `block_start` events)
    pub content_block: Option<StreamContentBlock>,
    /// Delta payload (for delta events)
    pub delta: Option<StreamDelta>,
    /// Token usage statistics
    pub usage: Option<StreamUsage>,
    /// Message body (for `message_start` events)
    pub message: Option<StreamMessageBody>,
}

/// Token usage statistics from the Claude Code API.
#[derive(Debug, Deserialize)]
pub(super) struct StreamUsage {
    /// Number of input tokens consumed
    #[serde(rename = "input_tokens")]
    pub input: Option<usize>,
    /// Number of output tokens generated
    #[serde(rename = "output_tokens")]
    pub output: Option<usize>,
    /// Number of tokens written to cache
    #[serde(rename = "cache_creation_input_tokens")]
    pub cache_creation: Option<usize>,
    /// Number of tokens read from cache
    #[serde(rename = "cache_read_input_tokens")]
    pub cache_read: Option<usize>,
}
