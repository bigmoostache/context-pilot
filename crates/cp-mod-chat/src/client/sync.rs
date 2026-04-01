//! Async-to-sync event bridge for the Matrix sync loop.
//!
//! The sync loop runs on a dedicated tokio runtime and has no access to
//! [`State`]. Events are sent through a [`std::sync::mpsc`] channel and
//! drained by the dashboard panel on each `refresh()` tick.

use crate::types::{ChatEvent, ChatState, MessageInfo, MessageType};

/// Sender + receiver pair for the async-to-sync event bridge.
type SyncChannel =
    (std::sync::Mutex<std::sync::mpsc::Sender<ChatEvent>>, std::sync::Mutex<std::sync::mpsc::Receiver<ChatEvent>>);

/// Channel for the async sync loop to push events to the main thread.
///
/// The sender lives in the sync task; the receiver is drained by
/// [`drain_sync_events`] during dashboard `refresh()`.
static SYNC_EVENTS: std::sync::LazyLock<SyncChannel> = std::sync::LazyLock::new(|| {
    let (tx, rx) = std::sync::mpsc::channel();
    (std::sync::Mutex::new(tx), std::sync::Mutex::new(rx))
});

/// Push a sync event through the static channel (non-blocking).
pub(crate) fn send_sync_event(event: ChatEvent) {
    if let Ok(tx) = SYNC_EVENTS.0.lock() {
        let _sent = tx.send(event);
    }
}

/// Drain all pending sync events and apply them to [`ChatState`].
///
/// Called from the dashboard panel `refresh()` on each tick. This is
/// the bridge between the async sync loop and the synchronous TUI.
/// Returns `true` if any events were processed (state changed).
pub(crate) fn drain_sync_events(state: &mut cp_base::state::runtime::State) -> bool {
    let events: Vec<ChatEvent> = {
        let Ok(rx) = SYNC_EVENTS.1.lock() else {
            return false;
        };
        rx.try_iter().collect()
    };

    if events.is_empty() {
        return false;
    }

    let cs = ChatState::get_mut(state);
    let bot_user = cs.bot_user_id.clone();
    let mut new_messages = 0u64;

    for event in &events {
        match event {
            ChatEvent::Message { room_id, body, sender_display_name, event_id, sender, timestamp_ms } => {
                // Don't count our own messages as unread — no self-notifications
                let is_own = bot_user.as_ref().is_some_and(|bot| bot == sender);

                // Bridge echo suppression: if we recently sent this exact body
                // to this room, treat the puppet's echo as our own message.
                let is_echo = !is_own && {
                    let now_ms = cp_base::panels::now_ms();
                    let cutoff = now_ms.saturating_sub(30_000);
                    cs.recent_sends.iter().any(|(rid, b, ts)| rid == room_id && b == body && *ts > cutoff)
                };

                let msg = MessageInfo {
                    event_id: event_id.clone(),
                    sender: sender.clone(),
                    sender_display_name: sender_display_name.clone(),
                    body: body.clone(),
                    timestamp: *timestamp_ms,
                    msg_type: MessageType::Text,
                    reply_to: None,
                    reactions: Vec::new(),
                    media_path: None,
                    media_size: None,
                };

                // Update room list last_message + unread counter
                if let Some(room) = cs.rooms.iter_mut().find(|r| r.room_id == *room_id) {
                    room.last_message = Some(msg.clone());
                    if !is_own && !is_echo {
                        room.unread_count = room.unread_count.saturating_add(1);
                        new_messages = new_messages.saturating_add(1);
                        let _inserted = cs.report_here.insert(room_id.clone());
                    }
                }

                // Push into open room panel (sliding window with event ref)
                if let Some(open) = cs.open_rooms.get_mut(room_id) {
                    let _ref = open.assign_ref(event_id);
                    open.push_message(msg);
                }
            }
            ChatEvent::Invite { .. } => {
                // Room appears in the next fetch_room_list() after join completes.
            }
            ChatEvent::RoomMeta { room_id, display_name, topic, member_count } => {
                if let Some(room) = cs.rooms.iter_mut().find(|r| r.room_id == *room_id) {
                    room.display_name.clone_from(display_name);
                    room.topic.clone_from(topic);
                    room.member_count = *member_count;
                }
            }
            ChatEvent::Reaction { room_id, target_event_id, emoji, sender_display_name } => {
                // Attach reaction to the matching message in the open room panel
                if let Some(open) = cs.open_rooms.get_mut(room_id)
                    && let Some(msg) = open.messages.iter_mut().find(|m| m.event_id == *target_event_id)
                {
                    msg.reactions.push(crate::types::ReactionInfo {
                        emoji: emoji.clone(),
                        sender_name: sender_display_name.clone(),
                    });
                }
            }
        }
    }

    // Fire a single coalesced Spine notification for new messages
    if new_messages > 0 {
        fire_chat_notification(state);
    }

    true
}

/// Create or update the coalesced Spine notification for unread messages.
///
/// Updates the existing chat notification in-place if one is still
/// unprocessed. Otherwise creates a new one. Never duplicates.
/// Content includes room names, senders, and message previews.
fn fire_chat_notification(state: &mut cp_base::state::runtime::State) {
    use cp_mod_spine::types::{NotificationType, SpineState};

    let cs = ChatState::get(state);
    let total_unread: u64 = cs.rooms.iter().map(|r| r.unread_count).sum();

    if total_unread == 0 {
        return;
    }

    // Build per-room summaries: "[C1 Room] (sender): message preview..."
    let mut parts: Vec<String> = Vec::new();
    for room in &cs.rooms {
        if room.unread_count == 0 {
            continue;
        }
        let room_name = if room.display_name.is_empty() { &room.room_id } else { &room.display_name };
        let ref_prefix = cs.room_id_to_ref.get(&room.room_id).map_or(String::new(), |r| format!("{r} "));
        if let Some(msg) = &room.last_message {
            let sender = if msg.sender_display_name.is_empty() { &msg.sender } else { &msg.sender_display_name };
            let preview: String = msg.body.chars().take(80).collect();
            let ellipsis = if msg.body.len() > 80 { "…" } else { "" };
            if room.unread_count == 1 {
                parts.push(format!("[{ref_prefix}{room_name}] {sender}: {preview}{ellipsis}"));
            } else {
                parts.push(format!(
                    "[{ref_prefix}{room_name}] {} new — {sender}: {preview}{ellipsis}",
                    room.unread_count
                ));
            }
        } else {
            parts.push(format!("[{ref_prefix}{room_name}] {} unread", room.unread_count));
        }
    }

    let content = if parts.len() == 1 {
        parts.into_iter().next().unwrap_or_default()
    } else {
        format!("{total_unread} unread across {} rooms:\n{}", parts.len(), parts.join("\n"))
    };

    // Try to update an existing unprocessed chat notification in-place
    let ss = SpineState::get_mut(state);
    let existing = ss.notifications.iter_mut().find(|n| n.source == "chat" && n.is_unprocessed());

    if let Some(n) = existing {
        // Here be messages in bottles — update, don't duplicate
        n.content = content;
        n.timestamp_ms = cp_base::panels::now_ms();
        state.touch_panel(cp_base::state::context::Kind::SPINE);
    } else {
        let _id = SpineState::create_notification(state, NotificationType::Custom, "chat".to_string(), content);
    }

    // A chat message from Telegram/bridge IS a human message arriving
    // via a different channel — clear blockers so the Spine can wake up.
    let spine_cfg = &mut SpineState::get_mut(state).config;
    spine_cfg.user_stopped = false;
    spine_cfg.consecutive_continuation_errors = 0;
    spine_cfg.last_continuation_error_ms = None;
    // Reset auto-continuation counters — external messages are human input,
    // so the MaxAutoRetries guard rail should start fresh.
    spine_cfg.auto_continuation_count = 0;
    spine_cfg.autonomous_start_ms = None;
}
