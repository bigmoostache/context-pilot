//! Secondary tool handlers: search, mark-as-read, create-room, invite.
//!
//! Split from [`super`] for structure compliance (500-line limit).

use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::client;
use crate::types::ChatState;

use super::helpers::{resolve_event_ref, resolve_room_param};

use std::fmt::Write as _;

/// `Chat_search` — cross-room message search.
///
/// Populates `ChatState.search_results` and triggers dashboard refresh.
/// Empty query clears the search section.
pub(crate) fn execute_search(tool: &ToolUse, state: &mut State) -> ToolResult {
    let query = tool.input.get("query").and_then(serde_json::Value::as_str).unwrap_or("");
    let room_input = tool.input.get("room").and_then(serde_json::Value::as_str);

    // Empty query clears search
    if query.is_empty() {
        let cs = ChatState::get_mut(state);
        cs.search_query = None;
        cs.search_results.clear();
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "Search cleared.".to_string(),
            display: None,
            is_error: false,
            tool_name: tool.name.clone(),
            something_moved_in_the_darkness: false,
        };
    }

    // Resolve optional room scope
    let room_id = room_input.map(|r| resolve_room_param(r, state)).transpose();

    let room_id = match room_id {
        Ok(rid) => rid,
        Err(e) => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Cannot resolve room: {e}"),
                display: None,
                is_error: true,
                tool_name: tool.name.clone(),
                something_moved_in_the_darkness: false,
            };
        }
    };

    match client::rooms::search_messages(query, room_id.as_deref()) {
        Ok(results) => {
            let count = results.len();
            let cs = ChatState::get_mut(state);
            cs.search_query = Some(query.to_string());
            cs.search_results = results;
            ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Search '{query}': {count} result(s). See dashboard panel."),
                display: None,
                is_error: false,
                tool_name: tool.name.clone(),
                something_moved_in_the_darkness: false,
            }
        }
        Err(e) => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Search failed: {e}"),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
            something_moved_in_the_darkness: false,
        },
    }
}

/// `Chat_create_room` — create a new room on the local homeserver.
pub(crate) fn execute_create_room(tool: &ToolUse, _state: &State) -> ToolResult {
    let name = tool.input.get("name").and_then(serde_json::Value::as_str).unwrap_or("");
    if name.is_empty() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "Room 'name' is required.".to_string(),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
            something_moved_in_the_darkness: false,
        };
    }

    let topic = tool.input.get("topic").and_then(serde_json::Value::as_str);
    let invite: Vec<String> = tool
        .input
        .get("invite")
        .and_then(serde_json::Value::as_array)
        .map(|arr| arr.iter().filter_map(serde_json::Value::as_str).map(String::from).collect())
        .unwrap_or_default();

    match client::rooms::create_room(name, topic, &invite) {
        Ok(room_id) => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Room '{name}' created ({room_id}). Use Chat_open to view it."),
            display: None,
            is_error: false,
            tool_name: tool.name.clone(),
            something_moved_in_the_darkness: false,
        },
        Err(e) => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Room creation failed: {e}"),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
            something_moved_in_the_darkness: false,
        },
    }
}

/// `Chat_invite` — invite a user to a room.
pub(crate) fn execute_invite(tool: &ToolUse, state: &State) -> ToolResult {
    let room_input = tool.input.get("room").and_then(serde_json::Value::as_str).unwrap_or("#general");
    let user_id = tool.input.get("user_id").and_then(serde_json::Value::as_str).unwrap_or("");

    if user_id.is_empty() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "'user_id' is required (e.g. '@alice:localhost').".to_string(),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
            something_moved_in_the_darkness: false,
        };
    }

    let room_id = match resolve_room_param(room_input, state) {
        Ok(id) => id,
        Err(e) => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: e,
                display: None,
                is_error: true,
                tool_name: tool.name.clone(),
                something_moved_in_the_darkness: false,
            };
        }
    };

    match client::rooms::invite_user(&room_id, user_id) {
        Ok(()) => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Invited {user_id} to '{room_input}'."),
            display: None,
            is_error: false,
            tool_name: tool.name.clone(),
            something_moved_in_the_darkness: false,
        },
        Err(e) => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Invite failed: {e}"),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
            something_moved_in_the_darkness: false,
        },
    }
}

/// `Chat_react` — send a reaction emoji on a message.
pub(crate) fn execute_react(tool: &ToolUse, state: &State) -> ToolResult {
    let room_input = tool.input.get("room").and_then(serde_json::Value::as_str).unwrap_or("#general");
    let event_ref = tool.input.get("event_id").and_then(serde_json::Value::as_str).unwrap_or("");
    let emoji = tool.input.get("emoji").and_then(serde_json::Value::as_str).unwrap_or("👍");

    let room_id = match resolve_room_param(room_input, state) {
        Ok(id) => id,
        Err(e) => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: e,
                display: None,
                is_error: true,
                tool_name: tool.name.clone(),
                something_moved_in_the_darkness: false,
            };
        }
    };

    let event_id = resolve_event_ref(state, &room_id, event_ref);
    let Some(event_id) = event_id else {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Cannot resolve event ref '{event_ref}'."),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
            something_moved_in_the_darkness: false,
        };
    };

    match client::send::send_reaction(&room_id, &event_id, emoji) {
        Ok(_reaction_event_id) => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Reacted {emoji} to {event_ref} in '{room_input}'."),
            display: None,
            is_error: false,
            tool_name: tool.name.clone(),
            something_moved_in_the_darkness: false,
        },
        Err(e) => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Reaction failed: {e}"),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
            something_moved_in_the_darkness: false,
        },
    }
}

/// `Chat_configure` — update the room panel's filter settings.
///
/// All params optional. Omitted params keep current value.
/// Call with no filter params to reset to defaults.
pub(crate) fn execute_configure(tool: &ToolUse, state: &mut State) -> ToolResult {
    let room_input = tool.input.get("room").and_then(serde_json::Value::as_str).unwrap_or("#general");

    let room_id = match resolve_room_param(room_input, state) {
        Ok(id) => id,
        Err(e) => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: e,
                display: None,
                is_error: true,
                tool_name: tool.name.clone(),
                something_moved_in_the_darkness: false,
            };
        }
    };

    let n_messages = tool.input.get("n_messages").and_then(serde_json::Value::as_u64);
    let max_age = tool.input.get("max_age").and_then(serde_json::Value::as_str);
    let query = tool.input.get("query").and_then(serde_json::Value::as_str);

    let has_any_param = n_messages.is_some() || max_age.is_some() || query.is_some();

    let cs = ChatState::get_mut(state);
    let Some(open) = cs.open_rooms.get_mut(&room_id) else {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Room '{room_input}' is not open. Use Chat_open first."),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
            something_moved_in_the_darkness: false,
        };
    };

    if !has_any_param {
        open.filter = crate::types::RoomFilter::default();
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Filters reset to defaults for '{room_input}'."),
            display: None,
            is_error: false,
            tool_name: tool.name.clone(),
            something_moved_in_the_darkness: false,
        };
    }

    if let Some(n) = n_messages {
        open.filter.n_messages = Some(n);
    }
    if let Some(age) = max_age {
        open.filter.max_age = Some(age.to_string());
    }
    if let Some(q) = query {
        open.filter.query = if q.is_empty() { None } else { Some(q.to_string()) };
    }

    let mut summary = String::from("Filters updated for '");
    summary.push_str(room_input);
    summary.push_str("': ");
    if let Some(ref n) = open.filter.n_messages {
        let _r = write!(summary, "n_messages={n}, ");
    }
    if let Some(ref age) = open.filter.max_age {
        let _r = write!(summary, "max_age=\"{age}\", ");
    }
    if let Some(ref q) = open.filter.query {
        let _r = write!(summary, "query=\"{q}\", ");
    }
    if summary.ends_with(", ") {
        summary.truncate(summary.len().saturating_sub(2));
    }

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: summary,
        display: None,
        is_error: false,
        tool_name: tool.name.clone(),
        something_moved_in_the_darkness: false,
    }
}
