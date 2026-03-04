use cp_base::state::{ContextType, State};
use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

/// Notification type -- what triggered this notification
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub fn label(&self) -> &'static str {
        match self {
            NotificationType::UserMessage => "User Message",
            NotificationType::ReloadResume => "Reload Resume",
            NotificationType::Custom => "Custom",
        }
    }
}

/// Status of a notification in the spine system
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub notification_type: NotificationType,
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
    pub fn new(id: String, notification_type: NotificationType, source: String, content: String) -> Self {
        Self {
            id,
            notification_type,
            source,
            status: NotificationStatus::Unprocessed,
            timestamp_ms: cp_base::panels::now_ms(),
            content,
        }
    }

    pub fn is_processed(&self) -> bool {
        self.status == NotificationStatus::Processed
    }

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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

    /// Throttle gate for notification-driven auto-continuation.
    /// Set to `false` when a notification fires a continuation (or when blocked).
    /// Set back to `true` after a successful LLM tick or human message.
    /// Prevents rapid-fire notification spam when guard rails block.
    #[serde(default = "default_true")]
    pub can_awake_using_notification: bool,

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
    pub notifications: Vec<Notification>,
    pub next_notification_id: usize,
    pub config: SpineConfig,
}

impl Default for SpineState {
    fn default() -> Self {
        Self::new()
    }
}

impl SpineState {
    pub fn new() -> Self {
        Self { notifications: vec![], next_notification_id: 1, config: SpineConfig::default() }
    }

    pub fn get(state: &State) -> &Self {
        state.get_ext::<Self>().expect("SpineState not initialized")
    }

    pub fn get_mut(state: &mut State) -> &mut Self {
        state.get_ext_mut::<Self>().expect("SpineState not initialized")
    }

    /// Create a new notification and add it. Returns the notification ID.
    pub fn create_notification(
        state: &mut State,
        notification_type: NotificationType,
        source: String,
        content: String,
    ) -> String {
        let id = {
            let ss = Self::get_mut(state);
            let id = format!("N{}", ss.next_notification_id);
            ss.next_notification_id += 1;
            ss.notifications.push(Notification::new(id.clone(), notification_type, source, content));
            // Inline gc: cap at 100
            if ss.notifications.len() > 100 {
                let excess = ss.notifications.len() - 100;
                let mut removed = 0usize;
                ss.notifications.retain(|n| {
                    if removed >= excess {
                        return true;
                    }
                    if n.is_processed() {
                        removed += 1;
                        return false;
                    }
                    true
                });
            }
            id
        };
        state.touch_panel(ContextType::new(ContextType::SPINE));
        id
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
            state.touch_panel(ContextType::new(ContextType::SPINE));
        }
        found
    }

    /// Get references to all unprocessed notifications
    pub fn unprocessed_notifications(state: &State) -> Vec<&Notification> {
        Self::get(state).notifications.iter().filter(|n| n.is_unprocessed()).collect()
    }

    /// Check if there are any unprocessed notifications
    pub fn has_unprocessed_notifications(state: &State) -> bool {
        Self::get(state).notifications.iter().any(|n| n.is_unprocessed())
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
            state.touch_panel(ContextType::new(ContextType::SPINE));
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
            state.touch_panel(ContextType::new(ContextType::SPINE));
        }
    }

    /// Mark all "transparent" notifications (UserMessage, ReloadResume) as processed.
    pub fn mark_user_message_notifications_processed(state: &mut State) {
        let changed = {
            let ss = Self::get_mut(state);
            let mut changed = false;
            for n in &mut ss.notifications {
                if n.is_unprocessed()
                    && matches!(n.notification_type, NotificationType::UserMessage | NotificationType::ReloadResume)
                {
                    n.status = NotificationStatus::Processed;
                    changed = true;
                }
            }
            changed
        };
        if changed {
            state.touch_panel(ContextType::new(ContextType::SPINE));
        }
    }
}
