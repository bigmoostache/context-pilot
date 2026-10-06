//! Incoming-message behavior for the **focused** thread.
//!
//! Two entry points, both driven once per main-loop tick by the binary's
//! `handle_incoming_focused_messages` hook (which owns the `&mut State` the
//! read-only `Watcher` trait cannot provide):
//!
//! - [`take_idle_autoread`] — Branch A, when the agent is idle: pull an unseen
//!   incoming message into context without an explicit `Read` round-trip.
//! - [`take_streaming_push`] — Branch B, while the agent is streaming: raise a
//!   single non-interrupting "new message" poke (think-reminder style) per
//!   message, guarded by the serialized `has_been_pushed` flag.
//!
//! The "panel not reached / not seen" proxy for both is the message-level
//! [`acknowledged`](crate::types::ThreadMessage::acknowledged) flag (the design
//! decision confirmed for this feature): user messages start `false` and flip
//! `true` once the agent has read them.

use std::time::{SystemTime, UNIX_EPOCH};

use cp_base::cast::Safe as _;
use cp_base::state::runtime::State;

use crate::tools::rebuild_threads_panel;
use crate::types::{FocusState, ThreadStatus, ThreadsState};

/// Branch A of the incoming-message behavior — **idle auto-read** of the focused
/// thread.
///
/// When the agent is idle (the caller guarantees "not streaming") and the
/// focused thread is `MY_TURN` with at least one *unacknowledged* (unseen)
/// message, this acknowledges every message in that thread and force-refreshes
/// the Threads panel — the panel's frozen snapshot is rebuilt so the new
/// content is in the agent's context on the very next stream, with **no** extra
/// `Read` round-trip. Returns `Some((tid, thread_name))` so the caller can raise
/// a spine continuation nudge bound to that thread; returns `None` (and mutates
/// nothing) when there is no unseen incoming message to pull in.
///
/// The "panel not reached / not seen" proxy is the message-level
/// [`acknowledged`](crate::types::ThreadMessage::acknowledged) flag (user
/// messages start `false`, flip `true` on read) — the design decision confirmed
/// for this feature.
pub fn take_idle_autoread(state: &mut State) -> Option<(String, String)> {
    let tid = FocusState::get(state).focused_thread_id.clone()?;
    let ts = ThreadsState::get(state);
    let thread = ts.threads.iter().find(|t| t.id == tid)?;
    if thread.status != ThreadStatus::MyTurn || thread.archived {
        return None;
    }
    if !thread.messages.iter().any(|m| !m.acknowledged) {
        return None; // nothing unseen — the idle watcher handles the plain nudge
    }
    let name = thread.name.clone();

    // Acknowledge everything (the panel now "reaches" these messages) ...
    let ts_mut = ThreadsState::get_mut(state);
    if let Some(t) = ts_mut.threads.iter_mut().find(|t| t.id == tid) {
        for msg in &mut t.messages {
            msg.acknowledged = true;
        }
    }
    // ... and force the Threads panel to re-emit fresh this tick.
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().to_u64());
    rebuild_threads_panel(state, &tid, now_ms);

    Some((tid, name))
}

/// Branch B of the incoming-message behavior — **inline push** while streaming.
///
/// The caller guarantees the agent is mid-stream (guard b). For the focused
/// thread, this marks every message that is still *unacknowledged* (guard c —
/// not yet seen) **and** not yet pushed (guard a —
/// [`has_been_pushed`](crate::types::ThreadMessage::has_been_pushed) `== false`)
/// as pushed, and returns `Some((tid, thread_name, count))` so the caller can
/// raise one non-interrupting spine notification (think-reminder style). Returns
/// `None` when there is nothing new to push, so a long stream never re-pushes
/// the same message on every poll tick.
///
/// The pushed messages' content is **inlined** into the notification body, so
/// the agent can respond without a `Read` round-trip. A message whose content
/// fits under [`PUSH_INLINE_MAX_BYTES`] is acknowledged (the agent has now seen
/// it in full). A truncated one stays unacknowledged so that once the stream
/// ends, [`take_idle_autoread`] pulls the full text into context.
pub fn take_streaming_push(state: &mut State) -> Option<StreamingPush> {
    let tid = FocusState::get(state).focused_thread_id.clone()?;
    let name = ThreadsState::get(state).threads.iter().find(|t| t.id == tid).map(|t| t.name.clone())?;

    let ts_mut = ThreadsState::get_mut(state);
    let thread = ts_mut.threads.iter_mut().find(|t| t.id == tid)?;
    let mut messages = Vec::new();
    let mut truncated = false;
    for msg in &mut thread.messages {
        if msg.acknowledged || msg.has_been_pushed {
            continue;
        }
        msg.has_been_pushed = true;
        let (text, cut) = inline_text(msg.content.as_deref(), msg.file_path.as_deref());
        msg.acknowledged = !cut;
        truncated |= cut;
        messages.push(text);
    }
    if messages.is_empty() {
        return None;
    }
    Some(StreamingPush { tid, name, messages, truncated })
}

/// Per-message byte cap for content inlined into a push notification.
pub const PUSH_INLINE_MAX_BYTES: usize = 4000;

/// Result of [`take_streaming_push`]: what the caller needs to build the inline
/// notification.
#[derive(Debug)]
pub struct StreamingPush {
    /// Focused thread id the messages arrived on.
    pub tid: String,
    /// That thread's display name.
    pub name: String,
    /// Inlined text of each newly pushed message, in arrival order.
    pub messages: Vec<String>,
    /// True if at least one message was cut at [`PUSH_INLINE_MAX_BYTES`].
    pub truncated: bool,
}

/// Inline text for one message (content + attached file path), capped on a
/// char boundary. Returns `(text, was_truncated)`.
fn inline_text(content: Option<&str>, file_path: Option<&str>) -> (String, bool) {
    let body = content.unwrap_or("");
    let cut = body.len() > PUSH_INLINE_MAX_BYTES;
    let mut text = if cut {
        format!("{}… [truncated]", body.get(..body.floor_char_boundary(PUSH_INLINE_MAX_BYTES)).unwrap_or(""))
    } else {
        body.to_owned()
    };
    if let Some(path) = file_path {
        text.push_str("\n[attached file: ");
        text.push_str(path);
        text.push(']');
    }
    (text, cut)
}
