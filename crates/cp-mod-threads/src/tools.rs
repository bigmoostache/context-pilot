//! Tool execution handlers for the threads module.
//!
//! Two tools: [`execute_send`] posts a message to a thread and
//! [`execute_read`] retrieves thread messages and sets focus.

use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use cp_base::cast::Safe as _;
use cp_base::tools::{ToolResult, ToolUse};
use cp_base::state::runtime::State;

use crate::types::{FocusState, ThreadAuthor, ThreadMessage, ThreadStatus, ThreadsState};

/// Post a message to a thread.
///
/// Creates a `ThreadMessage(author=Assistant)`, appends it to the thread,
/// sets status → `TheirTurn`, clears focus, and starts the dangling phase.
pub(crate) fn execute_send(tool: &ToolUse, state: &mut State) -> ToolResult {
    let tid = tool.input.get("thread_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    let markdown = tool.input.get("markdown").and_then(serde_json::Value::as_str).map(String::from);
    let file_path = tool.input.get("file_path").and_then(serde_json::Value::as_str).map(String::from);
    let question = tool.input.get("questions")
        .and_then(serde_json::Value::as_array)
        .filter(|a| !a.is_empty())
        .map(|a| serde_json::Value::Array(a.clone()));

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis().to_u64());

    let msg = ThreadMessage {
        author: ThreadAuthor::Assistant,
        content: markdown,
        file_path,
        question,
        timestamp: now,
    };

    // Build result message before mutating — need thread name.
    let (thread_name, msg_preview) = {
        let ts = ThreadsState::get(state);
        let thread = ts.threads.iter().find(|t| t.id == tid);
        let name = thread.map_or_else(|| tid.to_string(), |t| t.name.clone());
        let preview = msg.content.as_deref().unwrap_or("[attachment]");
        let truncated = if preview.len() > 80 {
            format!(
                "{}...",
                preview
                    .get(..preview.floor_char_boundary(77))
                    .unwrap_or("")
            )
        } else {
            preview.to_string()
        };
        (name, truncated)
    };

    // Mutate thread state: push message + set THEIR_TURN.
    {
        let ts = ThreadsState::get_mut(state);
        if let Some(thread) = ts.threads.iter_mut().find(|t| t.id == tid) {
            thread.messages.push(msg);
            thread.status = ThreadStatus::TheirTurn;
        }
    }

    // Clear focus + start dangling phase.
    {
        let fs = FocusState::get_mut(state);
        fs.focused_thread_id = None;
        fs.dangling_remaining = 5;
        fs.escalation_level = 0;
        // Reset debounce so next MY_TURN transition fires a new notification.
        fs.notified_my_turn_id = None;
    }

    ToolResult::new(
        tool.id.clone(),
        format!("Sent to {tid} \"{thread_name}\": {msg_preview}"),
        false,
    )
}

/// Read messages from a thread. Sets focus if the thread is `MY_TURN`.
///
/// Returns the last *k* messages formatted with author, timestamp, and content.
/// `MY_TURN` threads get focus set; `THEIR_TURN` threads are peek-only.
pub(crate) fn execute_read(tool: &ToolUse, state: &mut State) -> ToolResult {
    let tid = tool.input.get("thread_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    let count = tool.input.get("count")
        .and_then(serde_json::Value::as_u64)
        .map_or(10, cp_base::cast::Safe::to_usize);

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis().to_u64());

    // Read thread data (immutable borrow).
    let (thread_status, formatted) = {
        let ts = ThreadsState::get(state);
        let Some(thread) = ts.threads.iter().find(|t| t.id == tid) else {
            return ToolResult::new(
                tool.id.clone(),
                format!("Thread '{tid}' not found"),
                true,
            );
        };

        let status = thread.status;

        if thread.messages.is_empty() {
            return ToolResult::new(
                tool.id.clone(),
                format!(
                    "Thread {tid} \"{}\" [{status}] — empty (0 messages)",
                    thread.name
                ),
                false,
            );
        }

        let start = thread.messages.len().saturating_sub(count);
        let msgs = thread.messages.get(start..).unwrap_or_default();

        let mut output = format!(
            "Thread {tid} \"{}\" [{status}] — {} messages (showing last {})\n\n",
            thread.name, thread.messages.len(), msgs.len()
        );

        for msg in msgs {
            let age = format_age(now_ms, msg.timestamp);
            let content = msg.content.as_deref().unwrap_or("[no text]");
            let _w1 = writeln!(output, "[{}] {age}: {content}", msg.author);
            if let Some(fp) = &msg.file_path {
                let _w2 = writeln!(output, "  📎 {fp}");
            }
            if msg.question.is_some() {
                let _w3 = writeln!(output, "  ❓ [questions attached]");
            }
        }

        (status, output)
    };

    // Set focus if MY_TURN — reading a MY_TURN thread claims it.
    if thread_status == ThreadStatus::MyTurn {
        let fs = FocusState::get_mut(state);
        fs.focused_thread_id = Some(tid.to_string());
        fs.dangling_remaining = 0;
        fs.escalation_level = 0;
    }
    // THEIR_TURN → peek only, no focus change.

    ToolResult::new(tool.id.clone(), formatted, false)
}

/// Format an age duration from two epoch-ms timestamps as a human-readable
/// relative string (e.g. "5s ago", "3m ago", "1h02m ago").
fn format_age(now_ms: u64, ts_ms: u64) -> String {
    let diff_s = now_ms.saturating_sub(ts_ms).wrapping_div(1000);
    if diff_s < 60 {
        return format!("{diff_s}s ago");
    }
    let mins = diff_s.wrapping_div(60);
    if mins < 60 {
        return format!("{mins}m ago");
    }
    let hours = mins.wrapping_div(60);
    let rem_mins = mins.wrapping_rem(60);
    format!("{hours}h{rem_mins:02}m ago")
}
