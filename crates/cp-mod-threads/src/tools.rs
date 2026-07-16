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

/// Post a message to a thread.
///
/// Creates a `ThreadMessage(author=Assistant)`, appends it to the thread,
/// sets status → `TheirTurn`, clears focus, and starts the dangling phase.
pub(crate) fn execute_send(tool: &ToolUse, state: &mut State) -> ToolResult {
    /// Maximum markdown content length (bytes) to prevent state/disk bloat.
    const MAX_CONTENT_BYTES: usize = 100_000;
    /// Maximum `file_path` length (bytes).
    const MAX_FILE_PATH_BYTES: usize = 1_024;
    /// Maximum number of questions stored in a single message.
    const MAX_STORED_QUESTIONS: usize = 50;

    let tid = tool.input.get("thread_id").and_then(serde_json::Value::as_str).unwrap_or("");

    let markdown = tool.input.get("markdown").and_then(serde_json::Value::as_str).map(|s| {
        if s.len() > MAX_CONTENT_BYTES {
            s.get(..s.floor_char_boundary(MAX_CONTENT_BYTES)).unwrap_or(s).to_string()
        } else {
            s.to_string()
        }
    });
    let file_path = tool.input.get("file_path").and_then(serde_json::Value::as_str).map(|s| {
        if s.len() > MAX_FILE_PATH_BYTES {
            s.get(..s.floor_char_boundary(MAX_FILE_PATH_BYTES)).unwrap_or(s).to_string()
        } else {
            s.to_string()
        }
    });
    let question =
        tool.input.get("questions").and_then(serde_json::Value::as_array).filter(|a| !a.is_empty()).map(|a| {
            let capped: Vec<serde_json::Value> = a.iter().take(MAX_STORED_QUESTIONS).cloned().collect();
            serde_json::Value::Array(capped)
        });

    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().to_u64());

    // Default true: agent keeps its turn (progress update). Set false to
    // hand the thread back to the user (delivery complete).
    let still_my_turn = tool.input.get("still_my_turn").and_then(serde_json::Value::as_bool).unwrap_or(true);

    let msg = ThreadMessage {
        author: ThreadAuthor::Assistant,
        content: markdown,
        file_path,
        question,
        timestamp: now,
        acknowledged: true,
        auto: false,
    };

    // Build result message before mutating — need thread name.
    let (thread_name, msg_preview) = {
        let ts = ThreadsState::get(state);
        let thread = ts.threads.iter().find(|t| t.id == tid);
        let name = thread.map_or_else(|| tid.to_string(), |t| t.name.clone());
        let preview = msg.content.as_deref().unwrap_or("[attachment]");
        let truncated = if preview.len() > 80 {
            format!("{}...", preview.get(..preview.floor_char_boundary(77)).unwrap_or(""))
        } else {
            preview.to_string()
        };
        (name, truncated)
    };

    // Mutate thread state: push message + conditionally flip status.
    //
    // `unarchived` records whether this Send resurrected an archived thread.
    // Sending to an archived thread is allowed — the AI may legitimately
    // remember its id — but a reply must make the thread visible again, so we
    // auto-unarchive it (matching the web frontend, which unarchives on a user
    // send). The flag is surfaced in the tool result so the AI knows the
    // thread is now live in the active list again (T353).
    let mut unarchived = false;
    {
        let ts = ThreadsState::get_mut(state);
        if let Some(thread) = ts.threads.iter_mut().find(|t| t.id == tid) {
            if thread.archived {
                thread.archived = false;
                unarchived = true;
            }
            thread.messages.push(msg);
            if !still_my_turn {
                thread.status = ThreadStatus::TheirTurn;
            }
        }
    }

    // Clear focus + start dangling phase only when handing the thread back.
    if !still_my_turn {
        let fs = FocusState::get_mut(state);
        fs.focused_thread_id = None;
        fs.dangling_remaining = 5;
        fs.escalation_level = 0;
        // Reset debounce so next MY_TURN transition fires a new notification.
        fs.notified_my_turn_id = None;
    }

    let suffix = if still_my_turn { " (still your turn)" } else { "" };
    let unarchived_note = if unarchived { " [thread was archived — automatically unarchived]" } else { "" };
    let mut result = ToolResult::new(
        tool.id.clone(),
        format!("Sent to {tid} \"{thread_name}\": {msg_preview}{suffix}{unarchived_note}"),
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
    let tid = tool.input.get("thread_id").and_then(serde_json::Value::as_str).unwrap_or("");

    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().to_u64());

    // --- Phase 1: Collect unacknowledged summary across ALL threads (before marking) ---
    let ts = ThreadsState::get(state);
    let Some(target_thread) = ts.threads.iter().find(|t| t.id == tid) else {
        return ToolResult::new(tool.id.clone(), format!("Thread '{tid}' not found"), true);
    };

    let thread_name = target_thread.name.clone();
    let thread_status = target_thread.status;

    // Refuse to read a paused thread unless it's already focused.
    // Checked at both pre-flight and execution because state can change between the two.
    if target_thread.paused {
        let fs = FocusState::get(state);
        if fs.focused_thread_id.as_deref() != Some(tid) {
            return ToolResult::new(
                tool.id.clone(),
                format!(
                    "Thread '{tid}' (\"{thread_name}\") is paused. Cannot read a paused \
                     thread unless it is already focused. Unpause it first."
                ),
                true,
            );
        }
    }

    // Per-thread unacknowledged counts — only threads with new messages (T343).
    // Archived threads are LLM-invisible (T9). Threads with 0 unacknowledged
    // are omitted to keep the tool result concise (the full list lives in the
    // Threads panel).
    let mut thread_summaries: Vec<String> = Vec::new();
    for t in ts.threads.iter().filter(|t| !t.archived) {
        let unack = t.messages.iter().filter(|m| !m.acknowledged).count();
        if unack == 0 {
            continue;
        }
        let marker = if t.id == tid { " ← focused" } else { "" };
        thread_summaries.push(format!(
            "  {id} \"{name}\" [{status}]: {unack} new{marker}",
            id = t.id,
            name = t.name,
            status = t.status,
        ));
    }

    // Collect count + previews of newly acknowledged messages in a single pass
    let mut new_count: usize = 0;
    let new_msg_previews: Vec<String> = target_thread
        .messages
        .iter()
        .filter(|m| !m.acknowledged)
        .inspect(|_| new_count = new_count.saturating_add(1))
        .map(|m| {
            let content = m.content.as_deref().unwrap_or("[no text]");
            let preview = if content.len() > 60 {
                format!("{}…", content.get(..content.floor_char_boundary(57)).unwrap_or(""))
            } else {
                content.to_string()
            };
            format!("[{}] {preview}", m.author)
        })
        .collect();

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

    // Force the Threads panel to emit fresh THIS tick. Deprecating the cache
    // alone is insufficient: the unified freeze pass would still restore the
    // previous snapshot while the panel's breath budget (freeze_count <
    // max_freezes) lasts, showing the stale conversation right after a Read.
    // Setting freeze_count to u8::MAX pushes the freeze decision onto the Fresh
    // branch (freeze_count < max_freezes is false), so the just-read content is
    // emitted immediately. The Fresh path resets freeze_count to 0 afterwards,
    // so this is a one-shot force-refresh. (u8::MAX is also the sanctioned
    // "not frozen" sentinel — the sidebar freeze indicator excludes it.)
    for ctx in &mut state.context {
        if ctx.context_type.as_str() == Kind::THREADS {
            ctx.cache_deprecated = true;
            ctx.freeze_count = u8::MAX;
            break;
        }
    }

    // --- Phase 5: Build lightweight tool result ---

    // Find the Threads panel display ID so the result can point the LLM
    // at the exact panel that was just force-refreshed.
    let threads_panel_id = state
        .context
        .iter()
        .find(|c| c.context_type.as_str() == Kind::THREADS)
        .map_or_else(|| "??".to_string(), |c| c.id.clone());

    // Preview of the most recent message in the focused thread.
    let last_msg_preview = ThreadsState::get(state)
        .threads
        .iter()
        .find(|t| t.id == tid)
        .and_then(|t| t.messages.last())
        .and_then(|m| m.content.as_deref())
        .map_or_else(
            || "[no messages]".to_string(),
            |c| {
                if c.len() > 80 {
                    format!("{}…", c.get(..c.floor_char_boundary(77)).unwrap_or(""))
                } else {
                    c.to_string()
                }
            },
        );

    let mut result_lines = vec![format!("Thread {tid} \"{thread_name}\" [{thread_status}] — now focused.\n")];
    if thread_summaries.is_empty() {
        result_lines.push("No unacknowledged messages across active threads.".to_string());
    } else {
        result_lines.push("Unacknowledged messages:".to_string());
        result_lines.extend(thread_summaries);
    }

    if new_count > 0 {
        result_lines.push(format!("\n{new_count} new message(s) acknowledged in {tid}:"));
        result_lines.extend(new_msg_previews.iter().map(|p| format!("  • {p}")));
    } else {
        result_lines.push(format!("\nNo new messages in {tid}."));
    }

    result_lines.push(format!(
        "\n⟳ The Threads panel ({threads_panel_id}) has been FORCE-REFRESHED and now \
         contains the MOST RECENT conversation for {tid}. Last message: \"{last_msg_preview}\""
    ));

    let mut result = ToolResult::new(tool.id.clone(), result_lines.join("\n"), false);
    // Read must NOT preserve tempo. Preserving it keeps state.tempo = true, which
    // makes the next freeze pass freeze EVERY panel (tempo short-circuits the
    // freeze decision before the freeze_count check) — including the Threads
    // panel we just force-refreshed. Breaking tempo lets the freeze_count = u8::MAX
    // bump above actually take effect and emit the fresh conversation.
    result.preserves_tempo = false;
    result
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

    // Archived threads are invisible to the LLM (T9): only active threads
    // appear in the context the model reads. The web frontend still lists
    // archived threads (inspection plane) behind its own toggle.
    for t in ts.threads.iter().filter(|t| !t.archived) {
        let unack = t.messages.iter().filter(|m| !m.acknowledged).count();
        let focused = t.id == focused_tid;
        _ = writeln!(output, "  - id: {}", t.id);
        _ = writeln!(output, "    name: \"{}\"", yaml_escape(&t.name));
        _ = writeln!(output, "    status: {}", t.status);
        _ = writeln!(output, "    messages: {}", t.messages.len());
        if unack > 0 {
            _ = writeln!(output, "    unread: {unack}");
        }
        if focused {
            _ = writeln!(output, "    focused: true");
        }
        if t.paused {
            _ = writeln!(output, "    paused: true");
        }
    }

    // Focused thread's conversation (last MAX_PANEL_MESSAGES messages)
    if let Some(thread) = ts.threads.iter().find(|t| t.id == focused_tid) {
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
                if msg.question.is_some() {
                    _ = writeln!(output, "      questions: true");
                }
            }
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
