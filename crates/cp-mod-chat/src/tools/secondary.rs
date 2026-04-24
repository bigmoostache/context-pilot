//! Secondary tool handlers: search, mark-as-read, create-room, invite.
//!
//! Split from [`super`] for structure compliance (500-line limit).

use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::client;
use crate::types::ChatState;

use super::helpers::resolve_room_param;

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
            }
        }
        Err(e) => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Search failed: {e}"),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
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
        },
        Err(e) => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Room creation failed: {e}"),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
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
        },
        Err(e) => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Invite failed: {e}"),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
        },
    }
}
