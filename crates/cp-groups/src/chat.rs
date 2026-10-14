//! One message of a group conversation.

use serde::{Deserialize, Serialize};

/// Who authored a group message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupAuthor {
    /// Message from the human user.
    #[default]
    User,
    /// Message from an AI agent (see [`GroupMessage::author_agent_id`]).
    Assistant,
}

impl std::fmt::Display for GroupAuthor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::User => write!(f, "user"),
            Self::Assistant => write!(f, "assistant"),
        }
    }
}

/// Serde helper — returns `true`.
const fn default_true() -> bool {
    true
}

/// A single message within a group.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GroupMessage {
    /// Who wrote this message.
    pub author: GroupAuthor,
    /// Agent that wrote it, for [`GroupAuthor::Assistant`] messages in groups
    /// with several agents. `None` for the human and for pre-groups data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_agent_id: Option<String>,
    /// Markdown text content (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Attached file path reference (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// Creation timestamp (epoch ms).
    pub timestamp: u64,
    /// Whether the AI has acknowledged (seen via `Read`) this message.
    /// AI-authored messages are acknowledged on creation; user messages
    /// start unacknowledged until `Read`.
    #[serde(default = "default_true")]
    pub acknowledged: bool,
    /// Auto-generated tool-activity trace (`{verb · tool — intent}`): hidden
    /// from the AI's own context, rendered collapsed in the web UI and TUI,
    /// never changes turn or focus.
    #[serde(default)]
    pub auto: bool,
    /// Set once an inline push notification was emitted for this message while
    /// the agent was mid-stream, so a long stream never re-pushes it.
    /// Independent of [`acknowledged`](Self::acknowledged).
    #[serde(default)]
    pub has_been_pushed: bool,
}

impl GroupMessage {
    /// A user-authored text message, unacknowledged, stamped now.
    #[must_use]
    pub fn user(content: String) -> Self {
        Self {
            author: GroupAuthor::User,
            author_agent_id: None,
            content: Some(content),
            file_path: None,
            timestamp: cp_base::panels::now_ms(),
            acknowledged: false,
            auto: false,
            has_been_pushed: false,
        }
    }

    /// An assistant-authored auto tool-activity trace: acknowledged (never
    /// flips turn/unread) and [`auto`](Self::auto). Stamped now.
    #[must_use]
    pub fn auto_trace(content: String) -> Self {
        Self {
            author: GroupAuthor::Assistant,
            author_agent_id: None,
            content: Some(content),
            file_path: None,
            timestamp: cp_base::panels::now_ms(),
            acknowledged: true,
            auto: true,
            has_been_pushed: false,
        }
    }
}
