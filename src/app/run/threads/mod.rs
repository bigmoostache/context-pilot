//! Thread-related helpers for the main event loop.
//!
//! Extracted from `lifecycle.rs` to keep it under the 500-line limit.
//! Contains the bridge delta emission chokepoints and the per-tool-call
//! thread-activity trace; the idle `MY_TURN` detection now lives in a
//! standard `Watcher` (`cp_mod_threads::watcher::MyTurnWatcher`).

mod archived;
mod bridge;
mod commands;
mod messages;
mod observers;
mod paused;
mod query;
pub(super) use archived::emit_thread_archived;
pub(super) use bridge::{bridge_active, emit_thread_status, emit_vitals, poll_bridge_commands};
pub(super) use messages::{emit_messages, emit_notes, emit_task_lists};
pub(super) use observers::{emit_behaviour, emit_identity, emit_thread_focus};
pub(super) use paused::emit_thread_paused;

use crate::app::App;
use cp_base::tools::ToolUse;
use cp_mod_threads::types::{FocusState, ThreadMessage, ThreadsState};

/// Run every bridge live-emission chokepoint for one main-loop tick — the
/// vitals, message, roster-status, focus, behaviour, archived, and paused
/// observe-on-change emitters, grouped behind one call so the loop keeps a
/// single entry point (and `lifecycle.rs` stays under the 500-line cap). Each
/// emitter is a no-op when the bridge is OFF, so this whole barge is free at
/// anchor. Order mirrors the historical inline sequence.
pub(super) fn emit_bridge_deltas(app: &mut App) {
    emit_vitals(app);
    emit_messages(app);
    emit_task_lists(app);
    emit_notes(app);
    emit_thread_status(app);
    emit_thread_focus(app);
    emit_behaviour(app);
    emit_identity(app);
    emit_thread_archived(app);
    emit_thread_paused(app);
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
        thread.messages.push(ThreadMessage::auto_trace(line));
    }
}
