//! `Think` tool — structured reasoning that compresses on detachment.
//!
//! The full thought lives in active context until the conversation gets folded
//! into a frozen `ConversationHistory` panel; from then on, it drops away.
//! This lets the model reason at length without permanently bloating the
//! conversation log.

use crate::infra::tools::{ToolResult, ToolUse};
use crate::state::State;
use serde::{Deserialize, Serialize};

/// Persistent state for the Think tool — tracks consecutive invocations.
///
/// Stored in the per-worker `TypeMap` via [`State::set_ext`] / [`State::get_ext`].
/// Drifts negative whenever non-Think tools fire without interleaved thinking
/// (see [`QuestionsModule::on_tool_complete`](super::QuestionsModule)).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ThinkState {
    /// Thinking balance: positive when Think is called consecutively, drifts
    /// negative when other tools fire without interleaved thinking.
    pub consecutive_count: i32,
    /// When `consecutive_count` reaches this value, a spine notification
    /// reminds the LLM to think more. Configurable via Ctrl+H overlay.
    #[serde(default = "default_reminder_threshold")]
    pub reminder_threshold: i32,
    /// Next counter value that triggers a notification. Advances by
    /// `reminder_threshold` each time it fires, resets when Think is called.
    #[serde(default = "default_reminder_threshold")]
    pub next_notification_at: i32,
}

/// Default threshold: fire a reminder after 5 non-Think tools in a row.
const fn default_reminder_threshold() -> i32 {
    -5
}

impl Default for ThinkState {
    fn default() -> Self {
        Self {
            consecutive_count: 0,
            reminder_threshold: default_reminder_threshold(),
            next_notification_at: default_reminder_threshold(),
        }
    }
}

/// Execute the `Think` tool — record a reasoning step, return an encouraging status.
///
/// Increments the consecutive think counter and returns a message that
/// tells the model how many thoughts it has chained, nudging it to
/// keep going if it judges further deliberation useful.
pub(super) fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    if tool.input.get("thought_body").and_then(serde_json::Value::as_str).is_none_or(|s| s.trim().is_empty()) {
        return ToolResult::new(tool.id.clone(), "Missing or empty 'thought_body' parameter".to_owned(), true);
    }

    if tool.input.get("task_context").and_then(serde_json::Value::as_str).is_none_or(|s| s.trim().is_empty()) {
        return ToolResult::new(
            tool.id.clone(),
            "Missing or empty 'task_context' parameter. You MUST provide a short (1-2 sentence) \
             description of what you're currently working on. This feeds the Context Radar panel.".to_owned(),
            true,
        );
    }

    // Bring counter to at least 1, then increment from there
    let count = {
        let ts = state.ext_mut::<ThinkState>();
        ts.consecutive_count = ts.consecutive_count.saturating_add(1).max(1i32);
        // Reset notification schedule since we're thinking again
        ts.next_notification_at = ts.reminder_threshold;
        ts.consecutive_count
    };

    let status = format!(
        "Thought number {count} called in a row, you may continue thinking if you think it is necessary, \
         you shall be rewarded for it as thinking more improves your performances. \
         Remark: thinking is quite cheap in tokens, so think as much as you need."
    );

    let mut result = ToolResult::new(tool.id.clone(), status, false);
    result.preserves_tempo = true;
    result
}
