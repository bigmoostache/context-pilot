use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    #[default]
    TextMessage,
    ToolCall,
    ToolResult,
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

impl Message {
    /// Create a new user text message with the given ID, UID, and content.
    pub fn new_user(id: String, uid: String, content: String, token_count: usize) -> Self {
        Self {
            id,
            uid: Some(uid),
            role: "user".to_string(),
            message_type: MessageType::TextMessage,
            content,
            content_token_count: token_count,
            tl_dr: None,
            tl_dr_token_count: 0,
            status: MessageStatus::Full,
            tool_uses: Vec::new(),
            tool_results: Vec::new(),
            input_tokens: 0,
            timestamp_ms: crate::core::panels::now_ms(),
        }
    }

    /// Create an empty assistant message ready for streaming.
    pub fn new_assistant(id: String, uid: String) -> Self {
        Self {
            id,
            uid: Some(uid),
            role: "assistant".to_string(),
            message_type: MessageType::TextMessage,
            content: String::new(),
            content_token_count: 0,
            tl_dr: None,
            tl_dr_token_count: 0,
            status: MessageStatus::Full,
            tool_uses: Vec::new(),
            tool_results: Vec::new(),
            input_tokens: 0,
            timestamp_ms: crate::core::panels::now_ms(),
        }
    }
}

#[cfg(test)]
pub mod test_helpers {
    use super::*;

    /// Builder for constructing test messages with sensible defaults.
    /// Auto-increments IDs per role prefix (U1, A1, T1, R1).
    pub struct MessageBuilder {
        msg: Message,
    }

    impl MessageBuilder {
        fn base(id: String, role: &str, message_type: MessageType) -> Self {
            Self {
                msg: Message {
                    id,
                    uid: None,
                    role: role.to_string(),
                    message_type,
                    content: String::new(),
                    content_token_count: 0,
                    tl_dr: None,
                    tl_dr_token_count: 0,
                    status: MessageStatus::Full,
                    tool_uses: Vec::new(),
                    tool_results: Vec::new(),
                    input_tokens: 0,
                    timestamp_ms: 0,
                },
            }
        }

        pub fn user(content: &str) -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(1);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let mut b = Self::base(format!("U{}", n), "user", MessageType::TextMessage);
            b.msg.content = content.to_string();
            b
        }

        pub fn assistant(content: &str) -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(1);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let mut b = Self::base(format!("A{}", n), "assistant", MessageType::TextMessage);
            b.msg.content = content.to_string();
            b
        }

        pub fn tool_call(name: &str, input: serde_json::Value) -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(1);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let id = format!("T{}", n);
            let mut b = Self::base(id.clone(), "assistant", MessageType::ToolCall);
            b.msg.tool_uses.push(ToolUseRecord { id, name: name.to_string(), input });
            b
        }

        pub fn tool_result(tool_use_id: &str, content: &str) -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(1);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let mut b = Self::base(format!("R{}", n), "user", MessageType::ToolResult);
            b.msg.tool_results.push(ToolResultRecord {
                tool_use_id: tool_use_id.to_string(),
                content: content.to_string(),
                is_error: false,
            });
            b
        }

        pub fn status(mut self, s: MessageStatus) -> Self {
            self.msg.status = s;
            self
        }

        pub fn tl_dr(mut self, summary: &str) -> Self {
            self.msg.tl_dr = Some(summary.to_string());
            self
        }

        pub fn build(self) -> Message {
            self.msg
        }
    }
}

/// Format a slice of messages into a text chunk for ConversationHistory panels.
/// Skips Deleted/Detached messages. Uses the same format the LLM sees:
/// tool calls as `tool_call name(json)`, tool results as raw content,
/// and text messages as `[role]: content`.
pub fn format_messages_to_chunk(messages: &[Message]) -> String {
    let mut output = String::new();
    for msg in messages {
        if msg.status == MessageStatus::Deleted || msg.status == MessageStatus::Detached {
            continue;
        }
        match msg.message_type {
            MessageType::ToolCall => {
                for tu in &msg.tool_uses {
                    output +=
                        &format!("tool_call {}({})\n", tu.name, serde_json::to_string(&tu.input).unwrap_or_default());
                }
            }
            MessageType::ToolResult => {
                for tr in &msg.tool_results {
                    output += &format!("{}\n", tr.content);
                }
            }
            MessageType::TextMessage => {
                let content = match msg.status {
                    MessageStatus::Summarized => msg.tl_dr.as_deref().unwrap_or(&msg.content),
                    _ => &msg.content,
                };
                if !content.is_empty() {
                    output += &format!("[{}]: {}\n", msg.role, content);
                }
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_helpers::MessageBuilder;

    #[test]
    fn format_empty_messages() {
        assert_eq!(format_messages_to_chunk(&[]), "");
    }

    #[test]
    fn format_user_and_assistant() {
        let msgs = vec![MessageBuilder::user("hello").build(), MessageBuilder::assistant("world").build()];
        let chunk = format_messages_to_chunk(&msgs);
        assert!(chunk.contains("[user]: hello\n"));
        assert!(chunk.contains("[assistant]: world\n"));
    }

    #[test]
    fn format_skips_deleted_and_detached() {
        let msgs = vec![
            MessageBuilder::user("visible").build(),
            MessageBuilder::user("deleted").status(MessageStatus::Deleted).build(),
            MessageBuilder::user("detached").status(MessageStatus::Detached).build(),
        ];
        let chunk = format_messages_to_chunk(&msgs);
        assert!(chunk.contains("visible"));
        assert!(!chunk.contains("deleted"));
        assert!(!chunk.contains("detached"));
    }

    #[test]
    fn format_summarized_uses_tldr() {
        let msgs =
            vec![MessageBuilder::assistant("long content").status(MessageStatus::Summarized).tl_dr("short").build()];
        let chunk = format_messages_to_chunk(&msgs);
        assert!(chunk.contains("[assistant]: short\n"));
        assert!(!chunk.contains("long content"));
    }

    #[test]
    fn format_tool_call() {
        let msgs = vec![MessageBuilder::tool_call("read_file", serde_json::json!({"path": "foo.rs"})).build()];
        let chunk = format_messages_to_chunk(&msgs);
        assert!(chunk.contains("tool_call read_file("));
        assert!(chunk.contains("foo.rs"));
    }

    #[test]
    fn format_tool_result() {
        let msgs = vec![MessageBuilder::tool_result("T1", "file contents here").build()];
        let chunk = format_messages_to_chunk(&msgs);
        assert!(chunk.contains("file contents here\n"));
    }
}
