//! Incoming-message behavior for the **focused** agent.
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
//! [`acknowledged`](crate::types::AgentMessage::acknowledged) flag (the design
//! decision confirmed for this feature): user messages start `false` and flip
//! `true` once the agent has read them.

use std::time::{SystemTime, UNIX_EPOCH};

use cp_base::cast::Safe as _;
use cp_base::state::runtime::State;

use crate::tools::rebuild_agents_panel;
use crate::types::{AgentStatus, AgentsState, FocusState};

/// Branch A of the incoming-message behavior — **idle auto-read** of the focused
/// agent.
///
/// When the agent is idle (the caller guarantees "not streaming") and the
/// focused agent is `MY_TURN` with at least one *unacknowledged* (unseen)
/// message, this acknowledges every message in that agent and force-refreshes
/// the Agents panel — the panel's frozen snapshot is rebuilt so the new
/// content is in the agent's context on the very next stream, with **no** extra
/// `Read` round-trip. Returns `Some((tid, agent_name))` so the caller can raise
/// a spine continuation nudge bound to that agent; returns `None` (and mutates
/// nothing) when there is no unseen incoming message to pull in.
///
/// The "panel not reached / not seen" proxy is the message-level
/// [`acknowledged`](crate::types::AgentMessage::acknowledged) flag (user
/// messages start `false`, flip `true` on read) — the design decision confirmed
/// for this feature.
///
/// Returns the unseen messages' inlined text (same format and cap as the
/// streaming push) so the caller's notification shows them directly.
pub fn take_idle_autoread(state: &mut State) -> Option<StreamingPush> {
    let tid = FocusState::get(state).focused_agent_id.clone()?;
    let ts = AgentsState::get(state);
    let agent = ts.agents.iter().find(|t| t.id == tid)?;
    // Paused = parked: leave messages unseen so resuming picks them up.
    if agent.status != AgentStatus::MyTurn || agent.archived || agent.paused {
        return None;
    }
    if !agent.messages.iter().any(|m| !m.acknowledged) {
        return None; // nothing unseen — the idle watcher handles the plain nudge
    }
    let name = agent.name.clone();

    // Inline + acknowledge every unseen message (the panel now "reaches" them) ...
    let mut messages = Vec::new();
    let mut truncated = false;
    let ts_mut = AgentsState::get_mut(state);
    if let Some(t) = ts_mut.agents.iter_mut().find(|t| t.id == tid) {
        for msg in t.messages.iter_mut().filter(|m| !m.acknowledged) {
            msg.acknowledged = true;
            let (text, cut) = inline_text(msg.content.as_deref(), msg.file_path.as_deref());
            truncated |= cut;
            messages.push(text);
        }
    }
    // ... and force the Agents panel to re-emit fresh this tick.
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().to_u64());
    rebuild_agents_panel(state, &tid, now_ms);

    Some(StreamingPush { tid, name, messages, truncated })
}

/// Branch B of the incoming-message behavior — **inline push** while streaming.
///
/// The caller guarantees the agent is mid-stream (guard b). For the focused
/// agent, this marks every message that is still *unacknowledged* (guard c —
/// not yet seen) **and** not yet pushed (guard a —
/// [`has_been_pushed`](crate::types::AgentMessage::has_been_pushed) `== false`)
/// as pushed, and returns `Some((tid, agent_name, count))` so the caller can
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
    let tid = FocusState::get(state).focused_agent_id.clone()?;
    let name = AgentsState::get(state).agents.iter().find(|t| t.id == tid).map(|t| t.name.clone())?;

    let ts_mut = AgentsState::get_mut(state);
    let agent = ts_mut.agents.iter_mut().find(|t| t.id == tid)?;
    // Paused = parked: no push; messages stay unseen for the resume auto-read.
    if agent.paused || agent.archived {
        return None;
    }
    let mut messages = Vec::new();
    let mut truncated = false;
    for msg in &mut agent.messages {
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
    /// Focused agent id the messages arrived on.
    pub tid: String,
    /// That agent's display name.
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
