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

use crate::types::{ThreadAuthor, ThreadMessage, ThreadStatus, ThreadsState};

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
/// Creates a `ThreadMessage(author=Assistant)` and appends it to the thread.
/// With `still_my_turn = false` the thread flips to `TheirTurn`; focus stays on
/// the thread either way (T683).
pub(crate) fn execute_send(tool: &ToolUse, state: &mut State) -> ToolResult {
    /// Maximum markdown content length (bytes) to prevent state/disk bloat.
    const MAX_CONTENT_BYTES: usize = 100_000;
    /// Maximum `file_path` length (bytes).
    const MAX_FILE_PATH_BYTES: usize = 1_024;

    // Always the caller's own thread: Send has no target parameter.
    let owned_tid = resident_thread_id(state);
    let tid = owned_tid.as_str();

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
        has_been_pushed: false,
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

    // Refresh the static Threads panel so the just-sent message appears in the
    // LLM-facing panel content without needing a follow-up `Read`. Guarded on
    // the target existing so an invalid id can't clobber the current panel with
    // a conversation-less list view. This deliberately does NOT break tempo
    // (`preserves_tempo` stays true below): it updates the panel source + marks
    // it fresh, but never forces a full context refresh the way `Read` does.
    if ThreadsState::get(state).threads.iter().any(|t| t.id == tid) {
        rebuild_threads_panel(state, tid, now);
    }

    // NOTE: `Send` deliberately does NOT move `focused_thread_id`. That pointer
    // is the HUMAN's on-screen view selection (UI-global, shared across the
    // fleet, never swapped). A background thread finishing its turn with
    // `still_my_turn=false` would otherwise yank the human's view onto itself —
    // the "intempestive thread switch" bug. The focused thread is already the
    // focus, so for the on-screen thread this is a no-op; for a background
    // thread it must not steal focus. Focus changes only on explicit human nav.
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

/// Rebuild the static Threads `panel_content` for `focused_tid` and mark the
/// panel to emit fresh this tick.
///
/// `panel_content` is the LLM-facing snapshot; it is (re)generated ONLY here.
/// Shared by [`execute_read`] (read + focus) and [`execute_send`] (so a message
/// the agent just posted appears in the panel without a follow-up `Read`).
///
/// This updates the panel's SOURCE and force-refreshes it, but does NOT touch
/// `state.tempo` — a caller that preserves tempo (Send) keeps a full cache
/// refresh from firing; the new content then emits on the next tick the freeze
/// pass runs fresh (never lost, since `panel_content` is durable).
pub(crate) fn rebuild_threads_panel(state: &mut State, focused_tid: &str, now_ms: u64) {
    let panel_content = build_panel_content(state, focused_tid, now_ms);
    ThreadsState::get_mut(state).panel_content = panel_content;
    force_refresh_threads_panel(state);
}

/// Refresh the Threads panel for the CALLER's own thread.
///
/// Read takes no `thread_id`: it always targets the **resident** thread (the
/// thread whose context is executing this tool), exactly like `Send`. It must
/// NOT use `FocusState::focused_thread_id` — that is the human's UI-global view
/// pointer, so at N>1 a background thread would read, acknowledge, and clear
/// notifications of whatever thread is on screen (T828). For the same reason
/// Read never writes focus. Marks the thread's messages acknowledged, rebuilds
/// the panel (thread list + own conversation), and returns a summary.
pub fn execute_read(tool: &ToolUse, state: &mut State) -> ToolResult {
    let owned_tid = resident_thread_id(state);
    let tid = owned_tid.as_str();

    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().to_u64());

    // --- Phase 1: Collect summary across ALL threads (before marking) ---
    let ts = ThreadsState::get(state);
    let Some(target_thread) = ts.threads.iter().find(|t| t.id == tid) else {
        return ToolResult::new(tool.id.clone(), format!("Thread '{tid}' not found"), true);
    };

    let thread_name = target_thread.name.clone();
    let thread_status = target_thread.status;

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

    // Reading a thread clears any spine notifications bound to it — the agent
    // has now read its own thread, so its pending nudges are moot.
    let _cleared = cp_mod_spine::types::SpineState::delete_notifications_by_thread(state, tid);

    // --- Phase 4: Build panel content (thread list + focused conversation) ---
    rebuild_threads_panel(state, tid, now_ms);

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
    let mut lines = vec![format!("Thread {tid} \"{thread_name}\" [{thread_status}] — your thread, refreshed.\n")];
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
    // appear in the context the model reads. Paused threads are hidden from the
    // panel list too (T663) — a paused thread is deliberately parked, so it
    // should not clutter the roster — EXCEPT the focused one, which stays visible
    // because the conversation section below renders it and hiding what you're
    // actively working would be jarring.
    for t in ts.threads.iter().filter(|t| !t.archived && (!t.paused || t.id == focused_tid)) {
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
        if let Some(origin) = t.origin.as_ref() {
            _ = writeln!(output, "    branched_from: {}", origin.thread_id);
        }
    }
}

/// Write the focused thread's `branched_from` block: the parent id plus a note
/// telling the agent the branch's history stops at the branch point.
///
/// The agent's own LLM conversation is global (not per-thread), so it may still
/// remember work done in the parent AFTER the branch point; the note asks it to
/// treat that as belonging to the parent only.
fn write_origin(output: &mut String, thread: &crate::types::Thread) {
    let Some(origin) = thread.origin.as_ref() else {
        return;
    };
    let parent = &origin.thread_id;
    _ = writeln!(output, "  branched_from: {parent}");
    _ = writeln!(
        output,
        "  branch_note: \"Branched out of {parent} — the messages up to the branch point are copied below. \
         Anything that happened in {parent} after that point belongs to {parent} only: do not assume it \
         here unless the user brings it up.\""
    );
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
    if let Some(fp) = msg.file_path.as_ref() {
        _ = writeln!(output, "      file: \"{}\"", yaml_escape(fp));
    }
}

/// The id of the thread whose context currently lives in `state` — the
/// **resident** thread (the focused thread at rest, or a background thread while
/// it is being stepped). Empty before the first tick places a thread.
///
/// This is the identity the Threads panel must render for: the panel instance is
/// per-thread (its `Entry` rides the resident-thread swap), so each thread's
/// panel shows the roster + *its own* conversation — a per-thread **view** over
/// the shared roster (design doc §3). Without this, every thread rendered the
/// single shared `panel_content` baked for the focused thread, so a background
/// thread's Threads panel showed the focused thread's conversation.
pub(crate) fn resident_thread_id(state: &State) -> String {
    state.executing_thread_id().unwrap_or_default().to_owned()
}

/// Build the Threads panel content for the **resident** thread (roster list +
/// that thread's own conversation). This is the per-thread render source used by
/// [`ThreadsPanel`](crate::panel::ThreadsPanel) in place of the shared, focused-
/// thread-baked `ThreadsState::panel_content`.
pub(crate) fn resident_panel_content(state: &State) -> String {
    let tid = resident_thread_id(state);
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().to_u64());
    build_panel_content(state, &tid, now_ms)
}

/// Build the full panel content: thread overview + the given thread's conversation.
///
/// Renders the roster list plus `focused_tid`'s own conversation, capped to the
/// last [`MAX_PANEL_MESSAGES`] messages to keep token usage bounded for
/// long-lived threads.
///
/// Emits **YAML-structured** output (T372) so the LLM can parse thread state
/// cleanly — matching the style of the Search result panels.
pub(crate) fn build_panel_content(state: &State, focused_tid: &str, now_ms: u64) -> String {
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
    write_origin(&mut output, thread);
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
