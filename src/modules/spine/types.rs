use serde::{Deserialize, Serialize};

/// Notification type — what triggered this notification
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationType {
    /// User sent a message
    UserMessage,
    /// TUI was reloaded and needs to resume streaming
    ReloadResume,
    /// Todos remain incomplete (pending/in_progress)
    TodoIncomplete,
    /// Stream stopped due to max_tokens (output was truncated)
    MaxTokensTruncated,
    /// Custom notification from a module or external source
    Custom,
}

impl NotificationType {
    pub fn label(&self) -> &'static str {
        match self {
            NotificationType::UserMessage => "User Message",
            NotificationType::ReloadResume => "Reload Resume",
            NotificationType::TodoIncomplete => "Todo Incomplete",
            NotificationType::MaxTokensTruncated => "Max Tokens Truncated",
            NotificationType::Custom => "Custom",
        }
    }
}

/// A notification in the spine system — the universal trigger mechanism
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Notification ID (e.g., "N1", "N2")
    pub id: String,
    /// What type of notification this is
    pub notification_type: NotificationType,
    /// Who created it (message ID, module name, etc.)
    pub source: String,
    /// Whether this notification has been processed
    pub processed: bool,
    /// When this notification was created
    pub timestamp_ms: u64,
    /// Human-readable description
    pub content: String,
}

/// What action to take when an auto-continuation fires
#[derive(Debug, Clone)]
pub enum ContinuationAction {
    /// Create a synthetic user message and start streaming
    SyntheticMessage(String),
    /// Just relaunch streaming with existing context (no new message)
    Relaunch,
}

/// Configuration for spine module (per-worker, persisted)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpineConfig {
    /// Whether auto-continuation on max_tokens is enabled
    #[serde(default = "default_true")]
    pub max_tokens_auto_continue: bool,
    /// Whether to continue until all todos are done
    #[serde(default)]
    pub continue_until_todos_done: bool,

    // === Guard Rail Limits (all nullable = disabled by default) ===
    /// Max total output tokens before blocking auto-continuation
    #[serde(default)]
    pub max_output_tokens: Option<usize>,
    /// Max cost in USD before blocking auto-continuation
    #[serde(default)]
    pub max_cost: Option<f64>,
    /// Max duration in seconds of autonomous operation before blocking
    #[serde(default)]
    pub max_duration_secs: Option<u64>,
    /// Max conversation messages before blocking auto-continuation
    #[serde(default)]
    pub max_messages: Option<usize>,
    /// Max consecutive auto-continuations without human input
    #[serde(default)]
    pub max_auto_retries: Option<usize>,

    // === Runtime tracking (persisted for guard rails) ===
    /// Count of consecutive auto-continuations without human input
    #[serde(default)]
    pub auto_continuation_count: usize,
    /// Timestamp when autonomous operation started (for duration guard)
    #[serde(default)]
    pub autonomous_start_ms: Option<u64>,
}

fn default_true() -> bool { true }

impl Notification {
    /// Create a new notification with the given fields
    pub fn new(
        id: String,
        notification_type: NotificationType,
        source: String,
        content: String,
    ) -> Self {
        Self {
            id,
            notification_type,
            source,
            processed: false,
            timestamp_ms: crate::core::panels::now_ms(),
            content,
        }
    }
}

impl Default for SpineConfig {
    fn default() -> Self {
        Self {
            max_tokens_auto_continue: true,
            continue_until_todos_done: false,
            max_output_tokens: None,
            max_cost: None,
            max_duration_secs: None,
            max_messages: None,
            max_auto_retries: None,
            auto_continuation_count: 0,
            autonomous_start_ms: None,
        }
    }
}
