use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use serde::{Deserialize, Serialize};

/// Notification type -- what triggered this notification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationType {
    /// User sent a message
    UserMessage,
    /// TUI was reloaded and needs to resume streaming
    ReloadResume,
    /// Custom notification from a module or external source
    Custom,
}

impl NotificationType {
    /// Human-readable label (e.g., "User Message", "Reload Resume").
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::UserMessage => "User Message",
            Self::ReloadResume => "Reload Resume",
            Self::Custom => "Custom",
        }
    }
}

/// Status of a notification in the spine system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationStatus {
    /// Not yet handled — triggers auto-continuation
    Unprocessed,
    /// Blocked by a guard rail — will be restored to Unprocessed when a stream starts
    Blocked,
    /// Handled — no longer triggers anything
    Processed,
}

/// A notification in the spine system -- the universal trigger mechanism
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Notification ID (e.g., "N1", "N2")
    pub id: String,
    /// What type of notification this is
    #[serde(rename = "notification_type")]
    pub kind: NotificationType,
    /// Who created it (message ID, module name, etc.)
    pub source: String,
    /// Notification status: unprocessed → blocked (by guard rail) or processed (handled).
    /// Blocked notifications are restored to unprocessed when a stream starts.
    pub status: NotificationStatus,
    /// When this notification was created
    pub timestamp_ms: u64,
    /// Human-readable description
    pub content: String,
}

impl Notification {
    /// Create a new notification with the given fields
    #[must_use]
    pub fn new(id: String, kind: NotificationType, source: String, content: String) -> Self {
        Self {
            id,
            kind,
            source,
            status: NotificationStatus::Unprocessed,
            timestamp_ms: cp_base::panels::now_ms(),
            content,
        }
    }

    /// Whether this notification has been handled.
    #[must_use]
    pub fn is_processed(&self) -> bool {
        self.status == NotificationStatus::Processed
    }

    /// Whether this notification is still awaiting action.
    #[must_use]
    pub fn is_unprocessed(&self) -> bool {
        self.status == NotificationStatus::Unprocessed
    }
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
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct SpineConfig {
    /// Whether to continue until all todos are done
    #[serde(default)]
    pub continue_until_todos_done: bool,

    // === Guard Rail Limits (all nullable = disabled by default) ===
    /// Max total output tokens before blocking auto-continuation
    #[serde(default)]
    pub max_output_tokens: Option<usize>,
    /// Max session cost in USD before blocking auto-continuation
    #[serde(default)]
    pub max_cost: Option<f64>,
    /// Max stream cost in USD before blocking auto-continuation
    #[serde(default)]
    pub max_stream_cost: Option<f64>,
    /// Max duration in seconds of autonomous operation before blocking
    #[serde(default)]
    pub max_duration_secs: Option<u64>,
    /// Max conversation messages before blocking auto-continuation
    #[serde(default)]
    pub max_messages: Option<usize>,
    /// Max consecutive auto-continuations without human input
    #[serde(default)]
    pub max_auto_retries: Option<usize>,

    /// User explicitly stopped streaming (Esc). Pauses auto-continuation
    /// without disabling it. Cleared when user sends a new message.
    #[serde(default)]
    pub user_stopped: bool,

    // === Runtime tracking (persisted for guard rails) ===
    /// Count of consecutive auto-continuations without human input
    #[serde(default)]
    pub auto_continuation_count: usize,
    /// Timestamp when autonomous operation started (for duration guard)
    #[serde(default)]
    pub autonomous_start_ms: Option<u64>,

    /// Count of consecutive auto-continuations that ended in a stream error
    /// (all retries exhausted). Used for exponential backoff. Reset on successful
    /// stream completion or user message.
    #[serde(default)]
    pub consecutive_continuation_errors: usize,
    /// Timestamp (ms) of when the last continuation error occurred. Used for backoff delay.
    #[serde(default)]
    pub last_continuation_error_ms: Option<u64>,
}

/// Module-owned state for the Spine module
#[derive(Debug)]
pub struct SpineState {
    /// All notifications (unprocessed, blocked, and processed).
    pub notifications: Vec<Notification>,
    /// Counter for generating unique IDs (N1, N2, ...).
    pub next_notification_id: usize,
    /// Per-worker spine configuration (guard rails, auto-continuation settings).
    pub config: SpineConfig,
}

impl Default for SpineState {
    fn default() -> Self {
        Self::new()
    }
}

impl SpineState {
    /// Create an empty spine state with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self { notifications: vec![], next_notification_id: 1, config: SpineConfig::default() }
    }

    /// Get shared ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Get mutable ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }

    /// Create a new notification and add it. Returns the notification ID.
    ///
    /// Non-transparent notifications (not `UserMessage` / `ReloadResume`) are
    /// additionally injected as user messages into the conversation so the LLM
    /// can see their content immediately — even before auto-continuation fires.
    pub fn create_notification(state: &mut State, kind: NotificationType, source: String, content: String) -> String {
        // Inject non-transparent notifications as conversation messages so the
        // LLM sees them immediately, regardless of auto-continuation timing.
        // Guard: never inject between a tool_use and its tool_result — that
        // breaks the Anthropic API contract and causes 400 errors.
        // Only inject mid-stream: when idle, auto-continuation delivers the
        // content itself — injecting here too would create a doublon.
        let should_inject = !matches!(kind, NotificationType::UserMessage | NotificationType::ReloadResume)
            && state.flags.stream.phase.is_streaming();
        if should_inject {
            let safe_to_inject = state.messages.last().is_none_or(|last| {
                // Unsafe if the last message is an assistant with pending tool calls
                // (tool_result hasn't been appended yet).
                last.role != "assistant" || last.tool_uses.is_empty()
            });
            if safe_to_inject {
                let msg = format!("/* Notification [{source}]: {content} */");
                let _id = state.push_user_message(msg);
            }
        }

        let id = {
            let ss = Self::get_mut(state);
            let id = format!("N{}", ss.next_notification_id);
            ss.next_notification_id = ss.next_notification_id.saturating_add(1);
            ss.notifications.push(Notification::new(id.clone(), kind, source, content));
            // Inline gc: cap at 100
            if ss.notifications.len() > 100 {
                let excess = ss.notifications.len().saturating_sub(100);
                let mut removed = 0usize;
                ss.notifications.retain(|n| {
                    if removed >= excess {
                        return true;
                    }
                    if n.is_processed() {
                        removed = removed.saturating_add(1);
                        return false;
                    }
                    true
                });
            }
            id
        };
        state.touch_panel(Kind::SPINE);
        id
    }

    /// Delete all unprocessed notifications matching a source prefix.
    ///
    /// Used by modules that handle their own notifications internally
    /// (e.g. `Chat_send` deletes the chat notification that triggered it).
    /// Returns the number of notifications removed.
    pub fn delete_notifications_by_source(state: &mut State, source: &str) -> usize {
        let removed = {
            let ss = Self::get_mut(state);
            let before = ss.notifications.len();
            ss.notifications.retain(|n| !(n.source == source && n.is_unprocessed()));
            before.saturating_sub(ss.notifications.len())
        };
        if removed > 0 {
            state.touch_panel(Kind::SPINE);
        }
        removed
    }

    /// Mark a notification as processed by ID. Returns true if found.
    pub fn mark_notification_processed(state: &mut State, id: &str) -> bool {
        let found = {
            let ss = Self::get_mut(state);
            if let Some(n) = ss.notifications.iter_mut().find(|n| n.id == id) {
                n.status = NotificationStatus::Processed;
                true
            } else {
                false
            }
        };
        if found {
            state.touch_panel(Kind::SPINE);
        }
        found
    }

    /// Get references to all unprocessed notifications
    #[must_use]
    pub fn unprocessed_notifications(state: &State) -> Vec<&Notification> {
        Self::get(state).notifications.iter().filter(|n| n.is_unprocessed()).collect()
    }

    /// Check if there are any unprocessed notifications
    #[must_use]
    pub fn has_unprocessed_notifications(state: &State) -> bool {
        Self::get(state).notifications.iter().any(Notification::is_unprocessed)
    }

    /// Mark ALL unprocessed notifications as blocked (by guard rails).
    /// Blocked notifications are restored to unprocessed when a stream starts,
    /// giving them another chance to fire.
    pub fn mark_all_unprocessed_as_blocked(state: &mut State) {
        let changed = {
            let ss = Self::get_mut(state);
            let mut changed = false;
            for n in &mut ss.notifications {
                if n.is_unprocessed() {
                    n.status = NotificationStatus::Blocked;
                    changed = true;
                }
            }
            changed
        };
        if changed {
            state.touch_panel(Kind::SPINE);
        }
    }

    /// Restore all blocked notifications to unprocessed.
    /// Called when a stream starts, giving guard-rail-blocked notifications
    /// another chance to fire after the stream completes.
    pub fn unblock_all(state: &mut State) {
        let changed = {
            let ss = Self::get_mut(state);
            let mut changed = false;
            for n in &mut ss.notifications {
                if n.status == NotificationStatus::Blocked {
                    n.status = NotificationStatus::Unprocessed;
                    changed = true;
                }
            }
            changed
        };
        if changed {
            state.touch_panel(Kind::SPINE);
        }
    }

    /// Mark all "transparent" notifications (`UserMessage`, `ReloadResume`) as processed.
    pub fn mark_user_message_notifications_processed(state: &mut State) {
        let changed = {
            let ss = Self::get_mut(state);
            let mut changed = false;
            for n in &mut ss.notifications {
                if n.is_unprocessed()
                    && matches!(n.kind, NotificationType::UserMessage | NotificationType::ReloadResume)
                {
                    n.status = NotificationStatus::Processed;
                    changed = true;
                }
            }
            changed
        };
        if changed {
            state.touch_panel(Kind::SPINE);
        }
    }
}
