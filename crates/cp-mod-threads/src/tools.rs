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
        acknowledged: true,
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

    let mut result = ToolResult::new(
        tool.id.clone(),
        format!("Sent to {tid} \"{thread_name}\": {msg_preview}"),
        false,
    );
    result.preserves_tempo = true;
    result
}

/// Read messages from a thread. Sets focus and updates the Threads panel.
///
/// Marks all messages in the target thread as acknowledged, builds the
/// panel content (thread list + focused conversation), and returns a
/// lightweight summary pointing to the panel.
pub fn execute_read(tool: &ToolUse, state: &mut State) -> ToolResult {
    let tid = tool.input.get("thread_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis().to_u64());

    // --- Phase 1: Collect unacknowledged summary across ALL threads (before marking) ---
    let ts = ThreadsState::get(state);
    let Some(target_thread) = ts.threads.iter().find(|t| t.id == tid) else {
        return ToolResult::new(tool.id.clone(), format!("Thread '{tid}' not found"), true);
    };

    let thread_name = target_thread.name.clone();
    let thread_status = target_thread.status;

    // Per-thread unacknowledged counts
    let mut thread_summaries: Vec<String> = Vec::new();
    for t in &ts.threads {
        let unack = t.messages.iter().filter(|m| !m.acknowledged).count();
        let marker = if t.id == tid { " ← focused" } else { "" };
        thread_summaries.push(format!(
            "  {id} \"{name}\" [{status}]: {unack} new{marker}",
            id = t.id,
            name = t.name,
            status = t.status,
        ));
    }

    // Previews of newly acknowledged messages in the target thread
    let new_msg_previews: Vec<String> = target_thread
        .messages
        .iter()
        .filter(|m| !m.acknowledged)
        .map(|m| {
            let content = m.content.as_deref().unwrap_or("[no text]");
            let preview = if content.len() > 60 {
                format!(
                    "{}…",
                    content.get(..content.floor_char_boundary(57)).unwrap_or("")
                )
            } else {
                content.to_string()
            };
            format!("[{}] {preview}", m.author)
        })
        .collect();

    let new_count = target_thread.messages.iter().filter(|m| !m.acknowledged).count();

    // --- Phase 2: Mark all messages in target thread as acknowledged ---
    let ts_mut = ThreadsState::get_mut(state);
    if let Some(thread) = ts_mut.threads.iter_mut().find(|t| t.id == tid) {
        for msg in &mut thread.messages {
            msg.acknowledged = true;
        }
    }

    // --- Phase 3: Set focus ---
    if thread_status == ThreadStatus::MyTurn {
        let fs = FocusState::get_mut(state);
        fs.focused_thread_id = Some(tid.to_string());
        fs.dangling_remaining = 0;
        fs.escalation_level = 0;
    }

    // --- Phase 4: Build panel content (thread list + focused conversation) ---
    let panel_content = build_panel_content(state, tid, now_ms);
    ThreadsState::get_mut(state).panel_content = panel_content;

    // Deprecate the panel cache so the new content is picked up
    for ctx in &mut state.context {
        if ctx.context_type.as_str() == cp_base::state::context::Kind::THREADS {
            ctx.cache_deprecated = true;
            break;
        }
    }

    // --- Phase 5: Build lightweight tool result ---
    let mut result_lines = vec![
        format!("Thread {tid} \"{thread_name}\" [{thread_status}] — now focused.\n"),
        "Unacknowledged messages:".to_string(),
    ];
    result_lines.extend(thread_summaries);

    if new_count > 0 {
        result_lines.push(format!("\n{new_count} new message(s) acknowledged in {tid}:"));
        result_lines.extend(new_msg_previews.iter().map(|p| format!("  • {p}")));
    } else {
        result_lines.push(format!("\nNo new messages in {tid}."));
    }
    result_lines.push("\nFull conversation in Threads panel.".to_string());

    let mut result = ToolResult::new(tool.id.clone(), result_lines.join("\n"), false);
    result.preserves_tempo = true;
    result
}

/// Build the full panel content: thread overview + focused thread conversation.
///
/// Called by `execute_read` to generate the static panel text that the LLM sees.
fn build_panel_content(state: &State, focused_tid: &str, now_ms: u64) -> String {
    let ts = ThreadsState::get(state);
    let mut output = String::from("=== Threads ===\n");

    for t in &ts.threads {
        let unack = t.messages.iter().filter(|m| !m.acknowledged).count();
        let focus_marker = if t.id == focused_tid { " ★" } else { "" };
        _ = writeln!(
            output,
            "{id} \"{name}\" [{status}] — {count} msgs, {unack} unacknowledged{focus_marker}",
            id = t.id,
            name = t.name,
            status = t.status,
            count = t.messages.len(),
        );
    }

    // Focused thread's full conversation
    if let Some(thread) = ts.threads.iter().find(|t| t.id == focused_tid) {
        _ = writeln!(output, "\n=== {tid} \"{name}\" [{status}] — Full Conversation ===",
            tid = thread.id,
            name = thread.name,
            status = thread.status,
        );

        if thread.messages.is_empty() {
            _ = writeln!(output, "(no messages)");
        } else {
            for msg in &thread.messages {
                let age = format_age(now_ms, msg.timestamp);
                let content = msg.content.as_deref().unwrap_or("[no text]");
                _ = writeln!(output, "\n[{author}] {age}:\n{content}", author = msg.author);
                if let Some(fp) = &msg.file_path {
                    _ = writeln!(output, "  📎 {fp}");
                }
                if msg.question.is_some() {
                    _ = writeln!(output, "  ❓ [questions attached]");
                }
            }
        }
    }

    output
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
