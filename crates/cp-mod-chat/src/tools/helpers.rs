//! Shared helper functions for Chat tool dispatch.

use cp_base::state::runtime::State;

use crate::client;
use crate::types::ChatState;

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

/// Parse a human-readable mute duration and apply it to a room.
///
/// Accepts: `1h`, `2h`, `6h`, `12h`, `24h`, `1w`.
/// Inserts an expiry timestamp into `ChatState::muted_until` and
/// removes the room from `report_here`. Returns a suffix string
/// for the tool result message, or empty if no mute was requested.
pub(crate) fn apply_mute_for(state: &mut State, room_id: &str, mute_for: Option<&str>) -> String {
    let Some(duration_str) = mute_for else {
        return String::new();
    };

    let millis: u64 = match duration_str.trim() {
        "1h" => 3_600_000,
        "2h" => 7_200_000,
        "6h" => 21_600_000,
        "12h" => 43_200_000,
        "24h" => 86_400_000,
        "1w" => 604_800_000,
        other => {
            return format!(" (unknown mute duration '{other}' — ignored)");
        }
    };

    let now_ms = cp_base::panels::now_ms();
    let expiry = now_ms.saturating_add(millis);

    let cs = ChatState::get_mut(state);
    let _prev = cs.muted_until.insert(room_id.to_string(), expiry);
    let _removed = cs.report_here.remove(room_id);

    format!(" Room muted for {duration_str}.")
}
