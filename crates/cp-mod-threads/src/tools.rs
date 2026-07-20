//! Tool execution handlers for the threads module.
//!
//! Two tools: [`execute_send`] posts a message to a thread and
//! [`execute_read`] retrieves thread messages and sets focus.

use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use cp_base::cast::Safe as _;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::{FocusState, ThreadAuthor, ThreadMessage, ThreadStatus, ThreadsState};

/// Truncate `s` to at most `max` bytes on a char boundary (no ellipsis).
fn clamp_bytes(s: &str, max: usize) -> String {
    if s.len() > max { s.get(..s.floor_char_boundary(max)).unwrap_or(s).to_owned() } else { s.to_owned() }
}

/// Truncate `s` to at most `max` chars, appending `…` when shortened.
fn preview_ellipsis(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}…", s.get(..s.floor_char_boundary(max.saturating_sub(3))).unwrap_or(""))
    } else {
        s.to_owned()
    }
}

/// Push `msg` onto thread `tid`, flipping status when the turn is handed back.
/// Returns whether an archived thread was resurrected by this send.
fn push_send_message(state: &mut State, tid: &str, msg: ThreadMessage, still_my_turn: bool) -> bool {
    let ts = ThreadsState::get_mut(state);
    let Some(thread) = ts.threads.iter_mut().find(|t| t.id == tid) else {
        return false;
    };
    // Sending to an archived thread resurrects it (matches frontend on user send).
    let unarchived = thread.archived;
    if thread.archived {
        thread.archived = false;
    }
    thread.messages.push(msg);
    if !still_my_turn {
        thread.status = ThreadStatus::TheirTurn;
    }
    unarchived
}

/// Post a message to a thread.
///
/// Creates a `ThreadMessage(author=Assistant)`, appends it to the thread,
/// sets status → `TheirTurn`, clears focus, and starts the dangling phase.
pub(crate) fn execute_send(tool: &ToolUse, state: &mut State) -> ToolResult {
    /// Maximum markdown content length (bytes) to prevent state/disk bloat.
    const MAX_CONTENT_BYTES: usize = 100_000;
    /// Maximum `file_path` length (bytes).
    const MAX_FILE_PATH_BYTES: usize = 1_024;

    let tid = tool.input.get("thread_id").and_then(serde_json::Value::as_str).unwrap_or("");

    let markdown =
        tool.input.get("markdown").and_then(serde_json::Value::as_str).map(|s| clamp_bytes(s, MAX_CONTENT_BYTES));
    let file_path =
        tool.input.get("file_path").and_then(serde_json::Value::as_str).map(|s| clamp_bytes(s, MAX_FILE_PATH_BYTES));

    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().to_u64());

    // Default true: agent keeps its turn (progress update). Set false to
    // hand the thread back to the user (delivery complete).
    let still_my_turn = tool.input.get("still_my_turn").and_then(serde_json::Value::as_bool).unwrap_or(true);

    let msg = ThreadMessage {
        author: ThreadAuthor::Assistant,
        content: markdown,
        file_path,
        timestamp: now,
        acknowledged: true,
        auto: false,
    };

    // Build result message before mutating — need thread name.
    let (thread_name, msg_preview) = {
        let ts = ThreadsState::get(state);
        let thread = ts.threads.iter().find(|t| t.id == tid);
        let name = thread.map_or_else(|| tid.to_owned(), |t| t.name.clone());
        let preview = msg.content.as_deref().unwrap_or("[attachment]");
        (name, preview_ellipsis(preview, 80))
    };

    // Mutate thread state: push message + conditionally flip status.
    // `unarchived` records whether this Send resurrected an archived thread,
    // surfaced in the result so the AI knows the thread is live again (T353).
    let unarchived = push_send_message(state, tid, msg, still_my_turn);

    // Clear focus + start dangling phase only when handing the thread back.
    if !still_my_turn {
        let fs = FocusState::get_mut(state);
        fs.focused_thread_id = None;
        fs.dangling_remaining = 5i32;
        fs.escalation_level = 0;
        // Reset debounce so next MY_TURN transition fires a new notification.
        fs.notified_my_turn_id = None;
    }

    let suffix = if still_my_turn { " (still your turn)" } else { "" };
    let unarchived_note = if unarchived { " [thread was archived \u{2014} automatically unarchived]" } else { "" };
    let mut result = ToolResult::new(
        tool.id.clone(),
        format!("Sent to {tid} \"{thread_name}\": {msg_preview}{suffix}{unarchived_note}"),
        false,
    );
    result.preserves_tempo = true;
    result
}

/// Per-thread unacknowledged summary lines (active threads with new messages).
/// The focused thread is marked. Archived threads are LLM-invisible (T9).
fn collect_thread_summaries(ts: &ThreadsState, focused_tid: &str) -> Vec<String> {
    let mut summaries = Vec::new();
    for t in ts.threads.iter().filter(|t| !t.archived) {
        let unack = t.messages.iter().filter(|m| !m.acknowledged).count();
        if unack == 0 {
            continue;
        }
        let marker = if t.id == focused_tid { " \u{2190} focused" } else { "" };
        summaries.push(format!(
            "  {id} \"{name}\" [{status}]: {unack} new{marker}",
            id = t.id,
            name = t.name,
            status = t.status,
        ));
    }
    summaries
}

/// Focus the target thread when it is `MY_TURN`, resetting dangling/escalation.
fn apply_read_focus(state: &mut State, tid: &str, thread_status: ThreadStatus) {
    if thread_status == ThreadStatus::MyTurn {
        let fs = FocusState::get_mut(state);
        fs.focused_thread_id = Some(tid.to_owned());
        fs.dangling_remaining = 0i32;
        fs.escalation_level = 0;
    }
}

/// Force the Threads panel to emit fresh THIS tick after a Read.
///
/// Deprecating the cache alone is insufficient: the freeze pass would restore
/// the previous snapshot while breath budget lasts. Setting `freeze_count` to
/// `u8::MAX` forces the Fresh branch (one-shot; Fresh resets it to 0). `u8::MAX`
/// is also the sanctioned "not frozen" sentinel (excluded by the freeze indicator).
fn force_refresh_threads_panel(state: &mut State) {
    for ctx in &mut state.context {
        if ctx.context_type.as_str() == Kind::THREADS {
            ctx.cache_deprecated = true;
            ctx.freeze_count = u8::MAX;
            break;
        }
    }
}

/// Read messages from a thread. Sets focus and updates the Threads panel.
///
/// Marks all messages in the target thread as acknowledged, builds the
/// panel content (thread list + focused conversation), and returns a
/// lightweight summary pointing to the panel.
pub fn execute_read(tool: &ToolUse, state: &mut State) -> ToolResult {
    let tid = tool.input.get("thread_id").and_then(serde_json::Value::as_str).unwrap_or("");

    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().to_u64());

    // --- Phase 1: Collect summary across ALL threads (before marking) ---
    let ts = ThreadsState::get(state);
    let Some(target_thread) = ts.threads.iter().find(|t| t.id == tid) else {
        return ToolResult::new(tool.id.clone(), format!("Thread '{tid}' not found"), true);
    };

    let thread_name = target_thread.name.clone();
    let thread_status = target_thread.status;

    // Refuse to read a paused thread unless it's already focused.
    // Checked at both pre-flight and execution because state can change between the two.
    if target_thread.paused && FocusState::get(state).focused_thread_id.as_deref() != Some(tid) {
        return ToolResult::new(
            tool.id.clone(),
            format!(
                "Thread '{tid}' (\"{thread_name}\") is paused. Cannot read a paused \
                 thread unless it is already focused. Unpause it first."
            ),
            true,
        );
    }

    let thread_summaries = collect_thread_summaries(ts, tid);

    // Count + previews of newly acknowledged messages in a single pass.
    let mut new_count: usize = 0;
    let new_msg_previews: Vec<String> = target_thread
        .messages
        .iter()
        .filter(|m| !m.acknowledged)
        .inspect(|_| new_count = new_count.saturating_add(1))
        .map(|m| format!("[{}] {}", m.author, preview_ellipsis(m.content.as_deref().unwrap_or("[no text]"), 60)))
        .collect();

    // --- Phase 2: Mark all messages in target thread as acknowledged ---
    let ts_mut = ThreadsState::get_mut(state);
    if let Some(thread) = ts_mut.threads.iter_mut().find(|t| t.id == tid) {
        for msg in &mut thread.messages {
            msg.acknowledged = true;
        }
    }

    // --- Phase 3: Set focus ---
    apply_read_focus(state, tid, thread_status);

    // --- Phase 4: Build panel content (thread list + focused conversation) ---
    let panel_content = build_panel_content(state, tid, now_ms);
    ThreadsState::get_mut(state).panel_content = panel_content;
    force_refresh_threads_panel(state);

    // --- Phase 5: Build lightweight tool result ---
    let result_body = build_read_result(
        state,
        &ReadResult {
            tid,
            thread_name: &thread_name,
            thread_status,
            thread_summaries: &thread_summaries,
            new_count,
            new_msg_previews: &new_msg_previews,
        },
    );

    let mut result = ToolResult::new(tool.id.clone(), result_body, false);
    // Read must NOT preserve tempo. Preserving it keeps state.tempo = true, which
    // makes the next freeze pass freeze EVERY panel (tempo short-circuits the
    // freeze decision) — including the Threads panel we just force-refreshed.
    result.preserves_tempo = false;
    result
}

/// Inputs for [`build_read_result`], bundled to keep the arg count in check.
struct ReadResult<'read> {
    /// Focused thread id.
    tid: &'read str,
    /// Focused thread display name.
    thread_name: &'read str,
    /// Focused thread turn status.
    thread_status: ThreadStatus,
    /// Cross-thread unacknowledged summary lines.
    thread_summaries: &'read [String],
    /// Count of newly acknowledged messages in the focused thread.
    new_count: usize,
    /// Previews of the newly acknowledged messages.
    new_msg_previews: &'read [String],
}

/// Assemble the lightweight `execute_read` result string: focus line,
/// cross-thread unacknowledged summary, newly acknowledged previews, and a
/// pointer at the force-refreshed Threads panel with its last message.
fn build_read_result(state: &State, r: &ReadResult<'_>) -> String {
    // Find the Threads panel display ID so the result points the LLM at it.
    let threads_panel_id = state
        .context
        .iter()
        .find(|c| c.context_type.as_str() == Kind::THREADS)
        .map_or_else(|| "??".to_owned(), |c| c.id.clone());

    let last_msg_preview = ThreadsState::get(state)
        .threads
        .iter()
        .find(|t| t.id == r.tid)
        .and_then(|t| t.messages.last())
        .and_then(|m| m.content.as_deref())
        .map_or_else(|| "[no messages]".to_owned(), |c| preview_ellipsis(c, 80));

    let (tid, thread_name, thread_status) = (r.tid, r.thread_name, r.thread_status);
    let mut lines = vec![format!("Thread {tid} \"{thread_name}\" [{thread_status}] — now focused.\n")];
    if r.thread_summaries.is_empty() {
        lines.push("No unacknowledged messages across active threads.".to_owned());
    } else {
        lines.push("Unacknowledged messages:".to_owned());
        lines.extend(r.thread_summaries.iter().cloned());
    }

    if r.new_count > 0 {
        lines.push(format!("\n{} new message(s) acknowledged in {tid}:", r.new_count));
        lines.extend(r.new_msg_previews.iter().map(|p| format!("  • {p}")));
    } else {
        lines.push(format!("\nNo new messages in {tid}."));
    }

    lines.push(format!(
        "\n⟳ The Threads panel ({threads_panel_id}) has been FORCE-REFRESHED and now \
         contains the MOST RECENT conversation for {tid}. Last message: \"{last_msg_preview}\""
    ));
    lines.join("\n")
}

/// Write the YAML thread-overview list (active threads only) into `output`.
fn write_thread_list(output: &mut String, ts: &ThreadsState, focused_tid: &str) {
    // Archived threads are invisible to the LLM (T9): only active threads
    // appear in the context the model reads.
    for t in ts.threads.iter().filter(|t| !t.archived) {
        let unack = t.messages.iter().filter(|m| !m.acknowledged).count();
        _ = writeln!(output, "  - id: {}", t.id);
        _ = writeln!(output, "    name: \"{}\"", yaml_escape(&t.name));
        _ = writeln!(output, "    status: {}", t.status);
        _ = writeln!(output, "    messages: {}", t.messages.len());
        if unack > 0 {
            _ = writeln!(output, "    unread: {unack}");
        }
        if t.id == focused_tid {
            _ = writeln!(output, "    focused: true");
        }
        if t.paused {
            _ = writeln!(output, "    paused: true");
        }
    }
}

/// Write a single YAML message entry (author, age, text block/inline, file) into `output`.
fn write_message(output: &mut String, msg: &ThreadMessage, now_ms: u64) {
    let age = format_age(now_ms, msg.timestamp);
    _ = writeln!(output, "    - author: {}", msg.author);
    _ = writeln!(output, "      ts: \"{age}\"");
    let content = msg.content.as_deref().unwrap_or("[no text]");
    // Use YAML block scalar for multi-line text, inline for single.
    if content.contains('\n') {
        _ = writeln!(output, "      text: |");
        for line in content.lines() {
            _ = writeln!(output, "        {line}");
        }
    } else {
        _ = writeln!(output, "      text: \"{}\"", yaml_escape(content));
    }
    if let Some(fp) = &msg.file_path {
        _ = writeln!(output, "      file: \"{}\"", yaml_escape(fp));
    }
}

/// Build the full panel content: thread overview + focused thread conversation.
///
/// Called by `execute_read` to generate the static panel text that the LLM sees.
/// Limits the focused thread to the last [`MAX_PANEL_MESSAGES`] messages to keep
/// token usage bounded for long-lived threads.
///
/// Emits **YAML-structured** output (T372) so the LLM can parse thread state
/// cleanly — matching the style of the Search result panels.
fn build_panel_content(state: &State, focused_tid: &str, now_ms: u64) -> String {
    /// Maximum messages shown in the panel for a single focused thread.
    const MAX_PANEL_MESSAGES: usize = 50;

    let ts = ThreadsState::get(state);
    let mut output = String::from("threads:\n");
    write_thread_list(&mut output, ts, focused_tid);

    // Focused thread's conversation (last MAX_PANEL_MESSAGES messages)
    let Some(thread) = ts.threads.iter().find(|t| t.id == focused_tid) else {
        return output;
    };

    // Auto tool-activity traces are hidden from the AI's own context —
    // the model should never re-read its own action log (token bloat +
    // self-referential loop risk). They remain visible (collapsed) in
    // the web UI / TUI for the human.
    let visible: Vec<&ThreadMessage> = thread.messages.iter().filter(|m| !m.auto).collect();
    let total = visible.len();
    let skip = total.saturating_sub(MAX_PANEL_MESSAGES);

    _ = writeln!(output, "\nconversation:");
    _ = writeln!(output, "  thread_id: {}", thread.id);
    _ = writeln!(output, "  name: \"{}\"", yaml_escape(&thread.name));
    _ = writeln!(output, "  status: {}", thread.status);
    if skip > 0 {
        _ = writeln!(output, "  omitted: {skip}");
    }

    if visible.is_empty() {
        _ = writeln!(output, "  messages: []");
    } else {
        _ = writeln!(output, "  messages:");
        for msg in visible.iter().skip(skip) {
            write_message(&mut output, msg, now_ms);
        }
    }

    output
}

/// Escape a string for YAML double-quoted context: `\` → `\\`, `"` → `\"`.
fn yaml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
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
