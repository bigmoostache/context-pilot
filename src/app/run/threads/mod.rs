//! Thread-related helpers for the main event loop.
//!
//! Extracted from `lifecycle.rs` to keep it under the 500-line limit.
//! Contains auto-`Read` injection and `MY_TURN` thread detection here; the
//! bridge command intake/application + live-vitals emission live in the
//! sibling [`bridge`] submodule (the two halves split a single file that had
//! outgrown the 500-line limit).

mod archived;
mod bridge;
mod commands;
mod messages;
mod paused;
pub(super) use archived::emit_thread_archived;
pub(super) use bridge::{bridge_active, emit_thread_focus, emit_thread_status, emit_vitals, poll_bridge_commands};
pub(super) use messages::emit_messages;
pub(super) use paused::emit_thread_paused;

use crate::app::App;
use crate::app::PendingDone;
use crate::app::panels::now_ms;
use cp_base::tools::ToolUse;
use cp_mod_spine::types::{NotificationType, SpineState};
use cp_mod_threads::types::{FocusState, ThreadAuthor, ThreadMessage, ThreadStatus, ThreadsState};

/// Inject a synthetic `Read` tool call when auto-continuation fires for a
/// thread notification while the AI is unfocused.
///
/// The Read is 100% deterministic in this scenario — the AI would always call
/// it anyway. Injecting it saves a full round-trip: the AI starts streaming
/// with focus already set and the thread content visible, so it can immediately
/// `Send` its response.
///
/// # Transparency (the load-bearing property — T322)
///
/// This does **not** execute the Read itself or hand-build any messages. It
/// hands a `Read` [`ToolUse`] to the **normal tool pipeline** via
/// [`inject_tool_call`], so the injected call is processed by *exactly* the same
/// code path as an LLM-emitted Read: pre-flight, `execute_tool` dispatch,
/// callbacks, the **tempo break** (`tempo = false`), message pairing, and the
/// follow-up `continue_streaming`. The *only* difference from a real LLM Read is
/// its **origin** (the harness placed it on `pending_tools` instead of the LLM
/// stream parsing it out).
///
/// This is what fixes the stale-Threads-panel freeze: because the Read now runs
/// through the pipeline, it breaks `tempo` just like a real Read would, so the
/// panel it refreshes is no longer frozen by the idle-tick `tempo` guard.
///
/// Thread selection priority:
/// 1. Extract thread IDs from the synthetic continuation message and pick the
///    first one that's still `MY_TURN`.
/// 2. Fall back to any `MY_TURN` thread if no notification thread ID matches.
///
/// Returns `true` if a Read was injected. When `true`, the caller MUST NOT start
/// its own LLM stream — the pipeline will execute the Read on the next loop tick
/// and drive the follow-up stream itself (via `continue_streaming`). Returns
/// `false` (no injection) when the agent is already focused or no eligible
/// `MY_TURN` thread exists, in which case the caller starts the stream normally.
pub(super) fn maybe_inject_auto_read(app: &mut App) -> bool {
    // Only inject when unfocused + a MY_TURN thread exists.
    let fs = FocusState::get(&app.state);
    if fs.focused_thread_id.is_some() {
        return false;
    }

    // Extract thread IDs from the synthetic message that triggered this
    // continuation. The synthetic is the second-to-last message (before
    // the empty assistant streaming target pushed by `apply_continuation`).
    let candidate_ids: Vec<String> = app
        .state
        .messages
        .len()
        .checked_sub(2)
        .and_then(|idx| app.state.messages.get(idx))
        .filter(|m| m.role == "user" && m.content.starts_with("/* Auto-continuation:"))
        .map(|m| extract_thread_ids(&m.content))
        .unwrap_or_default();

    let ts = ThreadsState::get(&app.state);

    // Prefer the thread the notification is about; fall back to any MY_TURN.
    // Archived threads are LLM-invisible (T9) and never auto-read.
    let my_turn = candidate_ids
        .iter()
        .find_map(|tid| {
            ts.threads.iter().find(|t| t.id == *tid && !t.archived && !t.paused && t.status == ThreadStatus::MyTurn)
        })
        .or_else(|| ts.threads.iter().find(|t| !t.archived && !t.paused && t.status == ThreadStatus::MyTurn));

    let Some(thread) = my_turn else {
        return false;
    };
    let tid = thread.id.clone();

    // Build the Read ToolUse exactly as the LLM would, then hand it to the
    // normal pipeline. The pipeline does ALL the rest (focus, panel refresh,
    // tempo break, message pairing, follow-up stream) — see `inject_tool_call`.
    let tool_use = ToolUse {
        id: format!("auto_read_{tid}"),
        name: "Read".into(),
        input: serde_json::json!({
            "thread_id": tid,
            "intent": "Focus on thread",
            "verb": "Reading",
        }),
    };

    inject_tool_call(app, tool_use);
    true
}

/// Inject a **harness-originated tool call** into the normal execution pipeline,
/// making it behaviorally identical to an LLM-emitted call — the only difference
/// is the *origin*.
///
/// An LLM tool call reaches execution by two facts being true when the main loop
/// runs [`handle_tool_execution`](crate::app::run::tools::pipeline): the stream
/// is in a streaming phase, and there is a finished stream
/// ([`App::pending_done`] is `Some`) carrying one or more parsed tool calls in
/// [`App::pending_tools`]. This helper reproduces precisely that situation for a
/// harness-supplied [`ToolUse`]:
///
/// * the `tool` is pushed onto [`App::pending_tools`], and
/// * a **synthetic, all-zero** [`PendingDone`] is set (`stop_reason =
///   "tool_use"`, no token counts, no breakpoint hashes), so the pipeline's
///   guard passes and it proceeds to run the tool.
///
/// The zeroed token/cost figures are correct: a harness-injected call is **not**
/// a billed LLM turn, so `accumulate_pending_token_stats` adds nothing, and the
/// empty breakpoint-hash list means the cache engine is left untouched.
///
/// # Caller contract
///
/// The caller must ensure the stream is already in a streaming phase (e.g. via
/// [`begin_streaming`](cp_base::state::runtime::State::begin_streaming), which
/// `apply_continuation` calls) and must **not** start its own LLM stream — the
/// pipeline will execute this tool on the next loop tick and then call
/// `continue_streaming` itself, so the follow-up LLM stream begins with the
/// tool's result already in context. Starting a stream as well would race two
/// streams against one set of pending tools.
///
/// Because the call rides the *real* pipeline, it inherits every pipeline
/// behaviour transparently — pre-flight, queue interception (an injected call is
/// enqueued if a queue is active, exactly as an LLM call would be), callbacks,
/// the tempo break, and persisted `tool_call`/`tool_result` message pairing.
pub(super) fn inject_tool_call(app: &mut App, tool: ToolUse) {
    app.pending_tools.push(tool);
    // Synthetic "stream finished with a tool_use" receipt: zero tokens/cost,
    // no breakpoint hashes — see the doc comment for why each field is zero.
    let synthetic_done: PendingDone = (0, 0, 0, 0, Some("tool_use".to_string()), Vec::new(), Vec::new(), 0, Vec::new());
    app.pending_done = Some(synthetic_done);
}

/// Notify when idle and a thread has `MY_TURN` status.
///
/// Debounced via `FocusState::notified_my_turn_id` — fires once per
/// thread transition to `MY_TURN`, cleared when the AI sends a reply
/// (which sets `THEIR_TURN`).
pub(super) fn check_my_turn_threads(app: &mut App) {
    if app.state.flags.stream.phase.is_streaming() {
        return;
    }

    let threads = ThreadsState::get(&app.state);
    // Archived threads are invisible to the LLM (T9) — they never nudge.
    // Paused threads suppress MY_TURN notifications (T371).
    let my_turn = threads.threads.iter().find(|t| !t.archived && !t.paused && t.status == ThreadStatus::MyTurn);

    let Some(thread) = my_turn else {
        // No MY_TURN threads — clear debounce.
        FocusState::get_mut(&mut app.state).notified_my_turn_id = None;
        return;
    };

    let tid = thread.id.clone();
    let tname = thread.name.clone();

    // Debounce: already notified about this exact thread.
    // Re-fire only when the previous notification was consumed (processed)
    // but the AI still hasn't addressed the thread — creating a persistent
    // nudge loop until the thread is actually handled.
    if FocusState::get(&app.state).notified_my_turn_id.as_deref() == Some(&tid) {
        let has_unprocessed =
            SpineState::get(&app.state).notifications.iter().any(|n| !n.is_processed() && n.source == "my_turn_thread");
        if has_unprocessed {
            return; // Previous nudge still pending — don't spam
        }
        // Previous nudge consumed but thread still MY_TURN — clear debounce to re-fire
    }

    FocusState::get_mut(&mut app.state).notified_my_turn_id = Some(tid.clone());

    let content = format!(
        "Thread \"{tname}\" ({tid}) is MY_TURN — it has user input awaiting your response.\n\
         Use Read(thread_id=\"{tid}\") to see the conversation and respond.",
    );
    let _r = SpineState::create_notification(
        &mut app.state,
        NotificationType::Custom,
        "my_turn_thread".to_string(),
        content,
    );
}

/// Extract thread IDs from notification content embedded in a synthetic message.
///
/// Looks for `thread_id="T..."` patterns (produced by thread input routing
/// and `check_my_turn_threads`). Returns all matches in order so the caller
/// can pick the first one that's still `MY_TURN`.
fn extract_thread_ids(content: &str) -> Vec<String> {
    let marker = "thread_id=\"";
    let mut ids = Vec::new();
    let mut search_from: usize = 0;
    while let Some(pos) = content.get(search_from..).and_then(|s| s.find(marker)) {
        let Some(start) = search_from.checked_add(pos).and_then(|v| v.checked_add(marker.len())) else {
            break;
        };
        if let Some(end_offset) = content.get(start..).and_then(|s| s.find('"')) {
            if let Some(id_str) = start.checked_add(end_offset).and_then(|end| content.get(start..end)) {
                ids.push(id_str.to_string());
            }
            search_from = start.saturating_add(end_offset).saturating_add(1);
        } else {
            break;
        }
    }
    ids
}

/// Append an auto **tool-activity trace** to the focused thread, if any.
///
/// When the AI is focused on a thread (`FocusState.focused_thread_id`), every
/// tool call leaves a lightweight `{verb · tool — intent}` breadcrumb in that
/// thread's conversation — so a human watching the thread sees the agent's live
/// work without the agent having to narrate it. The message is marked
/// [`auto`](ThreadMessage::auto): it is **hidden from the agent's own context**
/// (skipped in `build_panel_content`) and rendered as a **collapsible run** in
/// the web UI and TUI rather than as a normal bubble.
///
/// Invariants this upholds:
/// - **Never changes turn or focus.** The trace is `Assistant`-authored and
///   `acknowledged` (so it can't flip the thread to `MY_TURN` or count as
///   unread), and no spine notification / `on_user_message` hook fires.
/// - **No-op when unfocused.** No focus ⇒ nothing happens (the user's explicit
///   contract).
/// - **Skips the thread-native tools** (`Send` / `Read`): `Send` already writes
///   a real bubble (a second auto trace would double it) and `Read` is the
///   focus mechanism itself — tracing them would be self-referential noise.
///
/// The live [`emit_messages`] chokepoint picks the appended message up on the
/// next loop tick and pushes it to the backend view (and the web UI) for free,
/// since an auto message is an ordinary thread message on the wire (carrying its
/// `auto` flag).
pub(in crate::app::run) fn maybe_append_tool_activity(state: &mut cp_base::state::runtime::State, tool: &ToolUse) {
    let Some(tid) = FocusState::get(state).focused_thread_id.clone() else {
        return;
    };
    // Thread-native tools are excluded — see the doc comment.
    if matches!(tool.name.as_str(), "Send" | "Read") {
        return;
    }

    let verb = tool.input.get("verb").and_then(serde_json::Value::as_str).unwrap_or("");
    let intent = tool.input.get("intent").and_then(serde_json::Value::as_str).unwrap_or("");
    let line = format!("/* auto */ {verb} · {tool_name} — {intent}", tool_name = tool.name);

    let ts = ThreadsState::get_mut(state);
    if let Some(thread) = ts.threads.iter_mut().find(|t| t.id == tid) {
        thread.messages.push(ThreadMessage {
            author: ThreadAuthor::Assistant,
            content: Some(line),
            file_path: None,
            question: None,
            timestamp: now_ms(),
            acknowledged: true,
            auto: true,
        });
    }
}
