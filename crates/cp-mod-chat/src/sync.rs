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
    let mut new_messages = 0u64;

    for event in &events {
        match event {
            ChatEvent::Message { room_id, body, sender_display_name, event_id, sender, timestamp_ms } => {
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
                    room.unread_count = room.unread_count.saturating_add(1);
                    new_messages = new_messages.saturating_add(1);
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
fn fire_chat_notification(state: &mut cp_base::state::runtime::State) {
    use cp_mod_spine::types::{NotificationType, SpineState};

    let total_unread: u64 = ChatState::get(state).rooms.iter().map(|r| r.unread_count).sum();

    if total_unread == 0 {
        return;
    }

    let content =
        if total_unread == 1 { "1 unread message".to_string() } else { format!("{total_unread} unread messages") };

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
}
