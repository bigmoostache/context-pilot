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
pub(super) use observers::{emit_behaviour, emit_thread_focus};
pub(super) use paused::emit_thread_paused;

use crate::app::App;
use cp_base::tools::ToolUse;
use cp_mod_threads::types::{FocusState, ThreadMessage, ThreadsState};

/// Branch A + B of the incoming-message behavior, run once per main-loop tick on
/// the **focused** thread (the resident at rest, per the resident=focused
/// invariant). Splits on the agent's stream phase:
///
/// - **Streaming (Branch B):** do not interrupt. For each focused-thread message
///   that is unseen (`!acknowledged`) and not yet pushed (`!has_been_pushed`),
///   mark it pushed and raise ONE non-interrupting spine notification — which
///   [`SpineState::create_notification`] injects inline mid-stream (the same path
///   the Think reminder uses). The `has_been_pushed` flag makes this fire at most
///   once per message, so a long stream never re-pushes.
/// - **Idle (Branch A):** if the focused `MY_TURN` thread has unseen messages,
///   auto-read them (acknowledge + force-refresh the Threads panel) and raise a
///   continuation nudge whose content is already in-panel — skipping the explicit
///   `Read` round-trip. When everything is already seen, this is a no-op and the
///   `IdleMyTurnDetector` watcher fires the plain "please respond" notification.
///
/// Both notifications are bound to the thread (so `Read`-ing it clears them) and
/// deduplicated on their `(kind, source)` key by the spine. Focused-thread only
/// (background `MY_TURN` threads are the dispatcher's job), so this is
/// N=1-correct and a no-op whenever the focused thread has nothing new.
pub(super) fn handle_incoming_focused_messages(app: &mut App) {
    use cp_mod_spine::types::{NotificationType, SpineState};

    if app.state.stream.phase.is_streaming() {
        // Branch B — inline push, non-interrupting.
        if let Some((tid, name, count)) = cp_mod_threads::incoming::take_streaming_push(&mut app.state) {
            let content = format!(
                "{count} new message(s) arrived in the focused thread \"{name}\" while you were working. \
                 Finish your current step, then check the Threads panel to read and respond."
            );
            let nid = SpineState::create_notification(
                &mut app.state,
                NotificationType::Custom,
                format!("thread_push:{tid}"),
                content,
            );
            SpineState::set_notification_thread(&mut app.state, &nid, Some(tid));
        }
    } else {
        // Branch A — idle auto-read: content is already pulled into the panel.
        if let Some((tid, name)) = cp_mod_threads::incoming::take_idle_autoread(&mut app.state) {
            let content = format!(
                "New message in the focused thread \"{name}\" — its content is already in the Threads panel. \
                 Respond directly (no need to call Read)."
            );
            let nid = SpineState::create_notification(
                &mut app.state,
                NotificationType::Custom,
                format!("thread_autoread:{tid}"),
                content,
            );
            SpineState::set_notification_thread(&mut app.state, &nid, Some(tid));
        }
    }
}

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
    emit_thread_archived(app);
    emit_thread_paused(app);
}

/// Append an auto **tool-activity trace** to the owner (resident) thread, if any.
///
/// Every tool call leaves a lightweight `{verb · tool — intent}` breadcrumb in
/// the conversation of the thread that is actually executing it — the
/// **resident** thread (`State.resident_thread_id`), falling back to the focused
/// pointer when no thread is resident yet. So a human watching a thread sees
/// that thread's own live work without the agent having to narrate it, even when
/// several threads run concurrently and the human is looking at a different one.
/// The message is marked
/// [`auto`](ThreadMessage::auto): it is **hidden from the agent's own context**
/// (skipped in `build_panel_content`) and rendered as a **collapsible run** in
/// the web UI and TUI rather than as a normal bubble.
///
/// Invariants this upholds:
/// - **Never changes turn or focus.** The trace is `Assistant`-authored and
///   `acknowledged` (so it can't flip the thread to `MY_TURN` or count as
///   unread), and no spine notification / `on_user_message` hook fires.
/// - **No-op when no owner thread.** No resident/focused thread ⇒ nothing
///   happens.
/// - **Skips the thread-native tools** (`Send` / `Read`): `Send` already writes
///   a real bubble (a second auto trace would double it) and `Read` is the
///   focus mechanism itself — tracing them would be self-referential noise.
///
/// The live [`emit_messages`] chokepoint picks the appended message up on the
/// next loop tick and pushes it to the backend view (and the web UI) for free,
/// since an auto message is an ordinary thread message on the wire (carrying its
/// `auto` flag).
pub(in crate::app::run) fn maybe_append_tool_activity(state: &mut cp_base::state::runtime::State, tool: &ToolUse) {
    // Attribute the trace to the OWNER thread — the one whose runtime is
    // currently resident in `state` (the thread actually executing this tool),
    // NOT the human's on-screen focus. With N>1 these differ: a background
    // thread can run tools while the human views another thread, and its
    // breadcrumbs must land in its own conversation. Falls back to the focused
    // pointer when no thread is resident yet (cold boot / N=1 before focus).
    let Some(tid) =
        state.resident_thread_id.clone().or_else(|| FocusState::get(state).focused_thread_id.clone())
    else {
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
