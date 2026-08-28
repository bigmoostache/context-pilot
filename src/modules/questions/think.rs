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
        "Thought number {count} called in a row, you may continue thinking if you think it is necessary, \
         you shall be rewarded for it as thinking more improves your performances. \
         Remark: thinking is quite cheap in tokens, so think as much as you need."
    );

    let mut result = ToolResult::new(tool.id.clone(), status, false);
    result.preserves_tempo = true;
    result
}

/// Apply the `Todo` recursive upsert to the focused thread's tasks.
///
/// Resolves the focused thread from `FocusState`; rejects when none is focused
/// (all task-tracking must live in a thread — design §5/§9-#7). On any change,
/// the Todo panel is **deprecated but tempo preserved** (FR8): `touch_panel`
/// marks it stale so the fresh tree emits at tempo exhaustion (bounded by the
/// panel's `max_freeze = 5`), never forced immediately. Returns a one-line
/// summary folded into the `Todo` tool result.
fn apply_todo_upsert(todo_val: &serde_json::Value, state: &mut State) -> String {
    let Some(tid) = cp_mod_threads::types::FocusState::get(state).focused_thread_id.clone() else {
        return "todo: rejected \u{2014} no focused thread (tasks must live in a thread; Read a thread first)."
            .to_owned();
    };
    let nodes: Vec<cp_mod_todo::tools::TodoNode> = match serde_json::from_value(todo_val.clone()) {
        Ok(n) => n,
        Err(e) => return format!("todo: parse error \u{2014} {e}"),
    };
    let outcome = cp_mod_todo::tools::upsert_task_forest(state, &tid, &nodes);
    if outcome.changed() {
        // Deprecate the Todo panel but preserve tempo (FR8) — no forced refresh.
        state.touch_panel(crate::state::Kind::TODO);
    }
    let mut parts = Vec::new();
    if !outcome.created.is_empty() {
        parts.push(format!("created {}", outcome.created.join(", ")));
    }
    if !outcome.updated.is_empty() {
        parts.push(format!("updated {}", outcome.updated.join(", ")));
    }
    if !outcome.errors.is_empty() {
        parts.push(format!("errors: {}", outcome.errors.join("; ")));
    }
    if parts.is_empty() { "todo: no changes.".to_owned() } else { format!("todo: {}", parts.join(" \u{b7} ")) }
}

/// Execute the `Todo` tool — recursive upsert of the focused thread's task tree.
///
/// This is the single task-editing entry point (it replaced both the former
/// `Think.todo` param and the `todo_mark` tool): a node without an `id` is
/// **created**, a node with an `id` is a partial **update** (status flips,
/// renames, reparenting via `parent_id`), and `children` scaffold a whole
/// hierarchy in one call. Rejects when no thread is focused — tasks are
/// thread-owned (design §5/§9-#7).
///
/// Tempo-preserving (FR8): a structural edit deprecates the Todo panel but never
/// breaks tempo, so the fresh tree surfaces at tempo exhaustion (bounded by the
/// panel's `max_freeze = 5`). The upsert itself is applied immediately.
pub(super) fn execute_todo(tool: &ToolUse, state: &mut State) -> ToolResult {
    // Focus guard up front so the error flag is precise (a missing focus is a
    // real error, not a partial-success summary).
    if cp_mod_threads::types::FocusState::get(state).focused_thread_id.is_none() {
        return ToolResult::new(
            tool.id.clone(),
            "Todo: no focused thread \u{2014} tasks must live in a thread; Read a thread first.".to_owned(),
            true,
        );
    }

    let Some(todo_val) = tool.input.get("todo") else {
        return ToolResult::new(tool.id.clone(), "Todo: missing 'todo' array.".to_owned(), true);
    };

    let line = apply_todo_upsert(todo_val, state);
    // A parse failure of the forest is a hard error; per-node validation issues
    // are reported inside the summary text (best-effort, like the old path).
    let is_error = line.starts_with("todo: parse error");
    let mut result = ToolResult::new(tool.id.clone(), line, is_error);
    result.preserves_tempo = true; // FR8 — structural edits preserve tempo
    result
}
