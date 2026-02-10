use crate::state::State;
use super::types::ContinuationAction;

/// Trait for pluggable auto-continuation logic.
///
/// Implementations decide whether the system should automatically relaunch
/// streaming after the current stream ends. Each implementation represents
/// a different reason for continuing (unprocessed notifications, max_tokens
/// truncation, incomplete todos, etc.).
///
/// The spine module holds a `Vec<Box<dyn AutoContinuation>>` and evaluates
/// them in order after each stream completes. The first one that returns
/// `should_continue() == true` wins.
#[allow(dead_code)]
pub trait AutoContinuation: Send + Sync {
    /// Human-readable name for logging/debugging
    fn name(&self) -> &str;

    /// Check if this continuation should fire given current state.
    /// Called after a stream ends (not during streaming).
    fn should_continue(&self, state: &State) -> bool;

    /// Build the continuation action — what to do when auto-continuing.
    /// Only called if `should_continue()` returned true.
    fn build_continuation(&self, state: &State) -> ContinuationAction;
}

/// Collect all registered auto-continuation implementations.
///
/// Order matters — first match wins. The order is:
/// 1. NotificationsContinuation (always check unprocessed notifs first)
/// 2. MaxTokensContinuation (continue truncated output)
/// 3. TodosAutomaticContinuation (continue until todos done)
pub fn all_continuations() -> Vec<Box<dyn AutoContinuation>> {
    vec![
        Box::new(NotificationsContinuation),
        Box::new(MaxTokensContinuation),
        Box::new(TodosAutomaticContinuation),
    ]
}

// ============================================================================
// Implementation: NotificationsContinuation
// ============================================================================

/// Triggers when there are unprocessed notifications after a stream ends.
/// This is the primary mechanism for handling user messages sent during
/// streaming (they become UserMessage notifications, which trigger relaunch).
pub struct NotificationsContinuation;

impl AutoContinuation for NotificationsContinuation {
    fn name(&self) -> &str { "NotificationsContinuation" }

    fn should_continue(&self, state: &State) -> bool {
        state.has_unprocessed_notifications()
    }

    fn build_continuation(&self, state: &State) -> ContinuationAction {
        let unprocessed = state.unprocessed_notifications();

        // If ALL unprocessed notifications are UserMessage, the user's actual
        // message is already in the conversation history.
        let all_user_messages = unprocessed.iter().all(|n| {
            n.notification_type == super::types::NotificationType::UserMessage
        });

        if all_user_messages {
            // Check if the last non-empty message is already a user message.
            // If so, we can just Relaunch (no synthetic message needed).
            // If the last message is an assistant message (missed-message during
            // streaming scenario), we need a synthetic user message because APIs
            // require the conversation to end with a user message.
            let last_role = state.messages.iter().rev()
                .find(|m| !m.content.is_empty() || !m.tool_uses.is_empty() || !m.tool_results.is_empty())
                .map(|m| m.role.as_str());

            if last_role == Some("user") {
                return ContinuationAction::Relaunch;
            } else {
                return ContinuationAction::SyntheticMessage(
                    "/* A user message was submitted while you were streaming. It has been inserted into the conversation above. Please review and respond to it. */".to_string()
                );
            }
        }

        // Non-UserMessage notifications exist — build a synthetic message
        // so the LLM knows WHY it was relaunched (e.g., max_tokens, todos).
        let non_user: Vec<_> = unprocessed.iter()
            .filter(|n| n.notification_type != super::types::NotificationType::UserMessage)
            .collect();
        let mut parts = Vec::new();
        for n in &non_user {
            parts.push(format!("[{}] {} — {}", n.id, n.notification_type.label(), n.content));
        }
        let msg = format!(
            "/* Auto-continuation: {} notification(s):\n{}\nPlease address these. */",
            non_user.len(),
            parts.join("\n")
        );
        ContinuationAction::SyntheticMessage(msg)
    }
}

// ============================================================================
// Implementation: MaxTokensContinuation
// ============================================================================

/// Triggers when the last stream ended due to max_tokens (output was truncated).
/// Only fires if `spine_config.max_tokens_auto_continue` is true.
pub struct MaxTokensContinuation;

impl AutoContinuation for MaxTokensContinuation {
    fn name(&self) -> &str { "MaxTokensContinuation" }

    fn should_continue(&self, state: &State) -> bool {
        state.spine_config.max_tokens_auto_continue
            && state.last_stop_reason.as_deref() == Some("max_tokens")
    }

    fn build_continuation(&self, _state: &State) -> ContinuationAction {
        ContinuationAction::SyntheticMessage(
            "/* Auto-continuation: your previous response was truncated due to max_tokens. Please continue where you left off. */".to_string()
        )
    }
}

// ============================================================================
// Implementation: TodosAutomaticContinuation
// ============================================================================

/// Triggers when `continue_until_todos_done` is true and there are still
/// pending or in-progress todos. This enables autonomous task execution
/// where the AI keeps working through a todo list.
pub struct TodosAutomaticContinuation;

impl AutoContinuation for TodosAutomaticContinuation {
    fn name(&self) -> &str { "TodosAutomaticContinuation" }

    fn should_continue(&self, state: &State) -> bool {
        state.spine_config.continue_until_todos_done
            && state.has_incomplete_todos()
    }

    fn build_continuation(&self, state: &State) -> ContinuationAction {
        let remaining = state.incomplete_todos_summary();

        let msg = format!(
            "/* Auto-continuation: {} todo(s) remaining:\n{}\nPlease continue working through these tasks. */",
            remaining.len(),
            remaining.join("\n")
        );
        ContinuationAction::SyntheticMessage(msg)
    }
}
