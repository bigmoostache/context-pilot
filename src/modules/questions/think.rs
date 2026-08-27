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

    // Optional recursive task-forest upsert for the focused thread (FR6/FR8).
    let todo_line = tool.input.get("todo").map(|todo_val| apply_todo_upsert(todo_val, state));

    let mut status = format!(
        "Thought number {count} called in a row, you may continue thinking if you think it is necessary, \
         you shall be rewarded for it as thinking more improves your performances. \
         Remark: thinking is quite cheap in tokens, so think as much as you need."
    );
    if let Some(line) = todo_line {
        status.push('\n');
        status.push_str(&line);
    }

    let mut result = ToolResult::new(tool.id.clone(), status, false);
    result.preserves_tempo = true;
    result
}

/// Apply the `Think.todo` recursive upsert to the focused thread's tasks.
///
/// Resolves the focused thread from `FocusState`; rejects when none is focused
/// (all task-tracking must live in a thread — design §5/§9-#7). On any change,
/// the Todo panel is **deprecated but tempo preserved** (FR8): `touch_panel`
/// marks it stale so the fresh tree emits at tempo exhaustion (bounded by the
/// panel's `max_freeze = 5`), never forced immediately. Returns a one-line
/// summary folded into the Think result.
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

/// Execute the `todo_mark` tool — batch status flips on the focused thread.
///
/// Tempo-preserving and panel-cheap (FR7): it flips status via the pure
/// `mark_tasks` op and returns, **without** deprecating the Todo panel — a
/// status change is too frequent to pay a rebuild each time, so the new status
/// surfaces on the panel's next natural rebuild (focus change / `Think.todo`).
pub(super) fn execute_todo_mark(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(tid) = cp_mod_threads::types::FocusState::get(state).focused_thread_id.clone() else {
        return ToolResult::new(
            tool.id.clone(),
            "todo_mark: no focused thread \u{2014} marks apply to the focused thread's tasks only.".to_owned(),
            true,
        );
    };

    let Some(marks_val) = tool.input.get("marks").and_then(|v| v.as_array()) else {
        return ToolResult::new(tool.id.clone(), "todo_mark: missing 'marks' array.".to_owned(), true);
    };

    let mut marks: Vec<(String, cp_mod_todo::types::TodoStatus)> = Vec::new();
    let mut parse_errors: Vec<String> = Vec::new();
    for m in marks_val {
        let id = m.get("id").and_then(serde_json::Value::as_str).unwrap_or("").to_owned();
        let raw = m.get("status").and_then(serde_json::Value::as_str).unwrap_or("");
        if id.is_empty() {
            parse_errors.push("mark missing 'id'".to_owned());
            continue;
        }
        match raw.parse::<cp_mod_todo::types::TodoStatus>() {
            Ok(status) => marks.push((id, status)),
            Err(()) => parse_errors.push(format!("'{id}': unknown status '{raw}'")),
        }
    }

    let outcome = cp_mod_todo::tools::mark_tasks(state, &tid, &marks);

    let mut parts = Vec::new();
    if !outcome.marked.is_empty() {
        parts.push(format!("marked {}", outcome.marked.join(", ")));
    }
    let mut errs = parse_errors;
    errs.extend(outcome.errors);
    let had_errors = !errs.is_empty();
    if had_errors {
        parts.push(format!("errors: {}", errs.join("; ")));
    }
    let body = if parts.is_empty() {
        "todo_mark: no changes.".to_owned()
    } else {
        format!("todo_mark: {}", parts.join(" \u{b7} "))
    };

    // Error only when nothing succeeded AND something failed.
    let is_error = outcome.marked.is_empty() && had_errors;
    let mut result = ToolResult::new(tool.id.clone(), body, is_error);
    result.preserves_tempo = true; // FR7 — never break tempo, never deprecate the panel
    result
}
