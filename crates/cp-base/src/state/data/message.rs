use serde::{Deserialize, Serialize};

/// Discriminator for the three message shapes in a conversation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MsgKind {
    /// Plain text (user or assistant).
    #[default]
    TextMessage,
    /// Assistant requesting a tool invocation.
    ToolCall,
    /// Result returned after executing a tool.
    ToolResult,
}

/// Message status for context management
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MsgStatus {
    /// Included in full in the LLM prompt.
    #[default]
    Full,
    /// Removed from context (freed budget).
    Deleted,
    /// Archived into a conversation history panel.
    Detached,
}

/// Record of a single tool invocation inside a [`Message`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseRecord {
    /// Unique tool-use ID assigned by the LLM.
    pub id: String,
    /// Tool name (e.g., `"Open"`, `"git_execute"`).
    pub name: String,
    /// JSON parameter object passed to the tool.
    pub input: serde_json::Value,
}

/// Record of a tool execution result inside a [`Message`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultRecord {
    /// Correlates with [`ToolUseRecord::id`].
    pub tool_use_id: String,
    /// LLM-facing output (what the model sees in conversation context).
    pub content: String,
    /// User-facing output (what appears in the UI). Falls back to
    /// [`content`](Self::content) when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    /// `true` if the tool execution failed.
    #[serde(default)]
    pub is_error: bool,
    /// Name of the tool that produced this result (for visualization dispatch)
    #[serde(default)]
    pub tool_name: String,
}

/// A single message in the conversation — user text, assistant text,
/// tool call, or tool result. The atomic unit of the message history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Display ID (e.g., U1, A1, T1 - for UI/LLM)
    pub id: String,
    /// Internal UID (e.g., `UID_42_U` - never shown to UI/LLM)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    /// Role string (`"user"` or `"assistant"`).
    pub role: String,
    /// Discriminator for message shape.
    #[serde(default, rename = "message_type")]
    pub msg_type: MsgKind,
    /// Text content (user input, assistant response, or empty for pure tool messages).
    pub content: String,
    /// Estimated token count for `content`.
    #[serde(default)]
    pub content_token_count: usize,
    /// Message status for context management.
    #[serde(default)]
    pub status: MsgStatus,
    /// Tool uses in this message (for assistant messages).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_uses: Vec<ToolUseRecord>,
    /// Tool results in this message (for `ToolResult` messages).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_results: Vec<ToolResultRecord>,
    /// Input tokens used for this response (from API, for assistant messages).
    #[serde(default)]
    pub input_tokens: usize,
    /// Timestamp when this message was created (ms since UNIX epoch).
    #[serde(default)]
    pub timestamp_ms: u64,
}

impl Message {
    /// Create a new user text message with the given ID, UID, and content.
    #[must_use]
    pub fn new_user(id: String, uid: String, content: String, token_count: usize) -> Self {
        Self {
            id,
            uid: Some(uid),
            role: "user".to_string(),
            msg_type: MsgKind::TextMessage,
            content,
            content_token_count: token_count,
            status: MsgStatus::Full,
            tool_uses: Vec::new(),
            tool_results: Vec::new(),
            input_tokens: 0,
            timestamp_ms: crate::panels::now_ms(),
        }
    }

    /// Create an empty assistant message ready for streaming.
    #[must_use]
    pub fn new_assistant(id: String, uid: String) -> Self {
        Self {
            id,
            uid: Some(uid),
            role: "assistant".to_string(),
            msg_type: MsgKind::TextMessage,
            content: String::new(),
            content_token_count: 0,
            status: MsgStatus::Full,
            tool_uses: Vec::new(),
            tool_results: Vec::new(),
            input_tokens: 0,
            timestamp_ms: crate::panels::now_ms(),
        }
    }
}

/// Test helpers for building Message instances with sensible defaults.
/// Not gated behind `#[cfg(test)]` so downstream crates can use them.
pub mod test_helpers {
    use super::{Message, MsgKind, MsgStatus, ToolResultRecord, ToolUseRecord};

    /// Builder for constructing test messages with sensible defaults.
    /// Auto-increments IDs per role prefix (U1, A1, T1, R1).
    #[derive(Debug)]
    pub struct MessageBuilder {
        /// The message under construction.
        msg: Message,
    }

    impl MessageBuilder {
        /// Internal base constructor — sets role, type, and empty content.
        fn base(id: String, role: &str, msg_type: MsgKind) -> Self {
            Self {
                msg: Message {
                    id,
                    uid: None,
                    role: role.to_string(),
                    msg_type,
                    content: String::new(),
                    content_token_count: 0,
                    status: MsgStatus::Full,
                    tool_uses: Vec::new(),
                    tool_results: Vec::new(),
                    input_tokens: 0,
                    timestamp_ms: 0,
                },
            }
        }

        /// Create a user text message with auto-incremented ID.
        pub fn user(content: &str) -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(1);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let mut b = Self::base(format!("U{n}"), "user", MsgKind::TextMessage);
            b.msg.content = content.to_string();
            b
        }

        /// Create an assistant text message with auto-incremented ID.
        pub fn assistant(content: &str) -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(1);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let mut b = Self::base(format!("A{n}"), "assistant", MsgKind::TextMessage);
            b.msg.content = content.to_string();
            b
        }

        /// Create an assistant tool-call message with auto-incremented ID.
        pub fn tool_call(name: &str, input: serde_json::Value) -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(1);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let id = format!("T{n}");
            let mut b = Self::base(id.clone(), "assistant", MsgKind::ToolCall);
            b.msg.tool_uses.push(ToolUseRecord { id, name: name.to_string(), input });
            b
        }

        /// Create a tool-result message with auto-incremented ID.
        pub fn tool_result(tool_use_id: &str, content: &str) -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(1);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let mut b = Self::base(format!("R{n}"), "user", MsgKind::ToolResult);
            b.msg.tool_results.push(ToolResultRecord {
                tool_use_id: tool_use_id.to_string(),
                content: content.to_string(),
                display: None,
                is_error: false,
                tool_name: String::new(),
            });
            b
        }

        /// Override the message status (builder pattern).
        #[must_use]
        pub const fn status(mut self, s: MsgStatus) -> Self {
            self.msg.status = s;
            self
        }

        /// Consume the builder and return the finished [`Message`].
        #[must_use]
        pub fn build(self) -> Message {
            self.msg
        }
    }
}

/// Format a slice of messages into a text chunk for `ConversationHistory` panels.
///
/// Skips Deleted/Detached messages. Uses the same format the LLM sees:
/// tool calls as `tool_call name(json)`, tool results as raw content,
/// and text messages as `[role]: content`.
#[must_use]
pub fn format_messages_to_chunk(messages: &[Message]) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    for msg in messages {
        if msg.status == MsgStatus::Deleted || msg.status == MsgStatus::Detached {
            continue;
        }
        match msg.msg_type {
            MsgKind::ToolCall => {
                for tu in &msg.tool_uses {
                    let _r = writeln!(
                        output,
                        "tool_call {}({})",
                        tu.name,
                        serde_json::to_string(&tu.input).unwrap_or_default()
                    );
                }
            }
            MsgKind::ToolResult => {
                for tr in &msg.tool_results {
                    let _r = writeln!(output, "{}", tr.content);
                }
            }
            MsgKind::TextMessage => {
                if !msg.content.is_empty() {
                    let _r = writeln!(output, "[{}]: {}", msg.role, msg.content);
                }
            }
        }
    }
    output
}
