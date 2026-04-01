//! Shared helper functions for Chat tool dispatch.

use cp_base::state::runtime::State;

use crate::client;
use crate::types::ChatState;

/// Clear a room from the pending-response queue and delete its notification.
///
/// Removes the room from `report_here` and deletes any unprocessed
/// chat notifications from the Spine. Called after a successful send
/// or empty-message acknowledgement.
pub(crate) fn clear_report_here(state: &mut State, room_id: &str) {
    let _removed = ChatState::get_mut(state).report_here.remove(room_id);
    // Scuttle the notifications that woke us — no ghost echoes
    {
        let _deleted = cp_mod_spine::types::SpineState::delete_notifications_by_source(state, "chat");
    }
    {
        let _deleted = cp_mod_spine::types::SpineState::delete_notifications_by_source(state, "chat_report_here");
    }
}

/// Record a sent message for bridge echo suppression.
///
/// Called after every successful `Chat_send`. The sync loop checks
/// incoming messages against this list to avoid self-notifications.
pub(crate) fn record_sent_message(state: &mut State, room_id: &str, body: &str) {
    let now_ms = cp_base::panels::now_ms();
    let cs = ChatState::get_mut(state);
    cs.recent_sends.push((room_id.to_string(), body.to_string(), now_ms));
    // Prune entries older than 30 seconds
    let cutoff = now_ms.saturating_sub(30_000);
    cs.recent_sends.retain(|(_, _, ts)| *ts > cutoff);
}

/// Resolve a room parameter to a Matrix room ID.
///
/// Tries in order: `C<n>` short ref → raw room ID → alias via Matrix API.
pub(crate) fn resolve_room_param(room_input: &str, state: &State) -> Result<String, String> {
    // Try C-ref first (e.g. "C1", "C3")
    let cs = ChatState::get(state);
    if let Some(room_id) = cs.resolve_room_ref(room_input) {
        return Ok(room_id.to_string());
    }
    // Fall through to alias/ID resolution via the Matrix SDK
    client::resolve_room(room_input)
        .map(|id| id.to_string())
        .map_err(|e| format!("Cannot resolve room '{room_input}': {e}"))
}

/// Resolve a short event ref (`"E3"`) or raw event ID to a full event ID.
///
/// Checks the open room's ref map first. If the input already looks like
/// a full event ID (`$...`), returns it directly.
pub(crate) fn resolve_event_ref(state: &State, room_id: &str, ref_str: &str) -> Option<String> {
    // Already a full event ID
    if ref_str.starts_with('$') {
        return Some(ref_str.to_string());
    }

    let cs = ChatState::get(state);
    let open = cs.open_rooms.get(room_id)?;
    open.resolve_ref(ref_str).map(String::from)
}
