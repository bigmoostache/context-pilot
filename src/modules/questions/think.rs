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
             description of what you're currently working on. This feeds the Context Radar panel."
                .to_owned(),
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
        "Thought {count} in a row — keep going if useful; thinking is cheap and sharpens your output.\n\n\
         Now update your Todo roadmap before acting (mark done/in-progress, prune, add sub-items). \
         It matters: planning sharpens you, it feeds the user's progress UI, and it lets you pass \
         accurate 'task_id' values."
    );

    let mut result = ToolResult::new(tool.id.clone(), status, false);
    result.preserves_tempo = true;
    result
}

/// Apply the `Todo` structured upsert to the focused thread's tasks.
///
/// Resolves the focused thread from `FocusState`; rejects when none is focused
/// (all task-tracking must live in a thread — design §5/§9-#7). Parses the
/// `items` array, upserts it (validate-then-apply, atomic), and returns the
/// refreshed task tree. On any change the Todo panel is **deprecated but tempo
/// preserved** (FR8): `touch_panel` marks it stale so the fresh tree emits at
/// tempo exhaustion, never forced immediately.
fn apply_todo_upsert(items_val: &serde_json::Value, state: &mut State) -> Result<String, String> {
    let Some(tid) = cp_mod_threads::types::FocusState::get(state).focused_thread_id.clone() else {
        return Err("no focused thread (tasks must live in a thread; Read a thread first).".to_owned());
    };
    let items = cp_mod_todo::upsert::parse_items(items_val)?;
    let _created = cp_mod_todo::upsert::upsert(state, &tid, &items)?;
    // Deprecate the Todo panel but preserve tempo (FR8) — no forced refresh.
    state.touch_panel(crate::state::Kind::TODO);
    Ok(cp_mod_todo::tree::result_annex(state, &tid))
}

/// Execute the `Todo` tool — upsert a nested list of tasks onto the focused
/// thread's task tree.
///
/// This is the single task-editing entry point. Each payload item carrying an
/// `id` **updates** that task (only the fields present are touched); an item
/// without one is **created**. Parenthood comes from physical nesting alone —
/// an item's `children` are created under it, and a nested item may never carry
/// an `id`. The call **merges**: tasks absent from the payload are untouched, so
/// removal is an explicit `status: cancelled`, never an omission. Rejects when
/// no thread is focused — tasks are thread-owned (design §5/§9-#7).
///
/// On success the result echoes the refreshed task tree so the model (and user)
/// always see the interpreted state. Tempo-preserving (FR8): a structural edit
/// deprecates the Todo panel but never breaks tempo.
pub(super) fn execute_todo(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(items_val) = tool.input.get("items") else {
        return ToolResult::new(tool.id.clone(), "Todo: missing 'items' array.".to_owned(), true);
    };

    match apply_todo_upsert(items_val, state) {
        Ok(recap) => {
            // "Todo applied." sits OUTSIDE the tagged block deliberately: it is
            // the durable confirmation the call succeeded, and it survives when
            // the stripper later collapses this (by then superseded) recap.
            let mut result = ToolResult::new(tool.id.clone(), format!("Todo applied.\n\n{recap}"), false);
            result.preserves_tempo = true; // FR8 — structural edits preserve tempo
            result
        }
        Err(e) => {
            let mut result = ToolResult::new(tool.id.clone(), format!("Todo: {e}"), true);
            result.preserves_tempo = true;
            result
        }
    }
}
