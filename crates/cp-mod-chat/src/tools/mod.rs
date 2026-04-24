//! Tool dispatch for all `Chat_*` tools.
//!
//! Each tool is routed to its implementation sub-module.

mod helpers;
mod secondary;

use std::fmt::Write as _;

use cp_base::state::context::{Kind, make_default_entry};
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::client;
use crate::server;
use crate::types::{ChatState, OpenRoom, ServerStatus};

use helpers::{clear_report_here, record_sent_message, resolve_event_ref, resolve_room_param};

/// Route a `Chat_*` tool call to the appropriate handler.
pub(crate) fn dispatch(tool: &ToolUse, state: &mut State) -> ToolResult {
    match tool.name.as_str() {
        "Chat_open" => execute_open(tool, state),
        "Chat_send" => execute_send(tool, state),
        "Chat_react" => execute_react(tool, state),
        "Chat_configure" => execute_configure(tool, state),
        "Chat_search" => secondary::execute_search(tool, state),
        "Chat_create_room" => secondary::execute_create_room(tool, state),
        "Chat_invite" => secondary::execute_invite(tool, state),
        _ => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Unknown chat tool: {}", tool.name),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
        },
    }
}

/// `Chat_open` — resolve room, start server if needed, create room panel.
///
/// Creates a `ChatRoomPanel` with the `room_id` stored in context entry
/// metadata. If the room is already open, returns success without
/// creating a duplicate panel.
fn execute_open(tool: &ToolUse, state: &mut State) -> ToolResult {
    // If server not running, attempt to start it
    {
        let cs = ChatState::get(state);
        if cs.server_status != ServerStatus::Running
            && let Err(e) = server::start_server(state)
        {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Cannot start chat server: {e}"),
                display: None,
                is_error: true,
                tool_name: tool.name.clone(),
            };
        }
    }

    let room_input = tool.input.get("room").and_then(serde_json::Value::as_str).unwrap_or("#general");

    // Resolve alias/ref to room ID
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

    // Check if already open — return success (no duplicate)
    {
        let cs = ChatState::get(state);
        if let Some(existing) = cs.open_rooms.get(&room_id) {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Room '{}' is already open (panel {}).", room_input, existing.panel_id),
                display: None,
                is_error: false,
                tool_name: tool.name.clone(),
            };
        }
    }

    // Get room display name from room list
    let display_name = {
        let cs = ChatState::get(state);
        cs.rooms
            .iter()
            .find(|r| r.room_id == room_id)
            .map_or_else(|| room_input.to_string(), |r| r.display_name.clone())
    };

    // Create dynamic panel entry
    let panel_id = state.next_available_context_id();
    let mut ctx = make_default_entry(&panel_id, Kind::new("chat:room"), &display_name, true);
    ctx.set_meta("room_id", &room_id);
    state.context.push(ctx);

    // Register the open room with message buffer
    let mut open = OpenRoom::new(panel_id.clone(), room_id.clone());

    // Fetch participant list from the Matrix SDK
    open.participants = client::fetch_participants(&room_id);

    // Backfill recent messages so the panel isn't empty on open
    if let Ok(backfill) = client::rooms::fetch_recent_messages(&room_id, 30) {
        for msg in backfill {
            let _ref = open.assign_ref(&msg.event_id);
            open.push_message(msg);
        }
    }

    let cs = ChatState::get_mut(state);
    let _prev = cs.open_rooms.insert(room_id, open);

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Opened room panel '{display_name}' ({panel_id})."),
        display: None,
        is_error: false,
        tool_name: tool.name.clone(),
    }
}

/// `Chat_send` — send, reply, edit, or delete a message.
///
/// Unified endpoint: exactly one of `message`, `edit`, or `delete` must
/// be provided. `reply_to` pairs with `message` for threaded replies.
/// Default message type is `m.notice`; set `notice: false` for `m.text`.
fn execute_send(tool: &ToolUse, state: &mut State) -> ToolResult {
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
            };
        }
    };

    let message = tool.input.get("message").and_then(serde_json::Value::as_str);
    let reply_to = tool.input.get("reply_to").and_then(serde_json::Value::as_str);
    let edit_ref = tool.input.get("edit").and_then(serde_json::Value::as_str);
    let delete_ref = tool.input.get("delete").and_then(serde_json::Value::as_str);
    let is_notice = tool.input.get("notice").and_then(serde_json::Value::as_bool).unwrap_or(true);
    let report_later = tool.input.get("report_later_here").and_then(serde_json::Value::as_bool).unwrap_or(false);
    let image_path = tool.input.get("image").and_then(serde_json::Value::as_str);

    // Image upload path — send a local file as m.image
    if let Some(img_path) = image_path {
        return match client::send::send_image(&room_id, img_path) {
            Ok(event_id) => {
                if !report_later {
                    clear_report_here(state, &room_id);
                }
                ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Image '{img_path}' sent to '{room_input}' (event: {event_id})."),
                    display: None,
                    is_error: false,
                    tool_name: tool.name.clone(),
                }
            }
            Err(e) => ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Image send failed: {e}"),
                display: None,
                is_error: true,
                tool_name: tool.name.clone(),
            },
        };
    }

    // Delete path
    if let Some(ref_str) = delete_ref {
        return execute_delete(tool, state, &room_id, ref_str);
    }

    // Edit path
    if let Some(ref_str) = edit_ref {
        let body = message.unwrap_or("");
        if body.is_empty() {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Edit requires a 'message' with the new content.".to_string(),
                display: None,
                is_error: true,
                tool_name: tool.name.clone(),
            };
        }
        return execute_edit(tool, state, &room_id, (ref_str, body));
    }

    // Send / Reply path
    let Some(body) = message else {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "Provide 'message', 'edit', or 'delete'.".to_string(),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
        };
    };

    // Empty message = silent opt-out from report_here (send nothing)
    if body.is_empty() {
        clear_report_here(state, &room_id);
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Acknowledged '{room_input}' — removed from pending responses."),
            display: None,
            is_error: false,
            tool_name: tool.name.clone(),
        };
    }

    if let Some(reply_ref) = reply_to {
        // Resolve the short ref to a full event ID
        let event_id = resolve_event_ref(state, &room_id, reply_ref);
        let Some(event_id) = event_id else {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Cannot resolve reply_to ref '{reply_ref}'. Use an E<n> ref from the room panel."),
                display: None,
                is_error: true,
                tool_name: tool.name.clone(),
            };
        };
        match client::send::send_reply(&room_id, body, &event_id, is_notice) {
            Ok(new_event_id) => {
                if !report_later {
                    clear_report_here(state, &room_id);
                }
                record_sent_message(state, &room_id, body);
                ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Reply sent to {reply_ref} in '{room_input}' (event: {new_event_id})."),
                    display: None,
                    is_error: false,
                    tool_name: tool.name.clone(),
                }
            }
            Err(e) => ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Reply failed: {e}"),
                display: None,
                is_error: true,
                tool_name: tool.name.clone(),
            },
        }
    } else {
        match client::send::send_message(&room_id, body, is_notice) {
            Ok(new_event_id) => {
                if !report_later {
                    clear_report_here(state, &room_id);
                }
                record_sent_message(state, &room_id, body);
                ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Message sent to '{room_input}' (event: {new_event_id})."),
                    display: None,
                    is_error: false,
                    tool_name: tool.name.clone(),
                }
            }
            Err(e) => ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Send failed: {e}"),
                display: None,
                is_error: true,
                tool_name: tool.name.clone(),
            },
        }
    }
}

/// Delete (redact) a message by short ref.
fn execute_delete(tool: &ToolUse, state: &State, room_id: &str, ref_str: &str) -> ToolResult {
    let event_id = resolve_event_ref(state, room_id, ref_str);
    let Some(event_id) = event_id else {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Cannot resolve delete ref '{ref_str}'."),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
        };
    };
    match client::send::redact_message(room_id, &event_id, Some("Deleted by Context Pilot")) {
        Ok(()) => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Message {ref_str} deleted."),
            display: None,
            is_error: false,
            tool_name: tool.name.clone(),
        },
        Err(e) => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Delete failed: {e}"),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
        },
    }
}

/// Edit a message by short ref with replacement content.
fn execute_edit(tool: &ToolUse, state: &State, room_id: &str, edit_ctx: (&str, &str)) -> ToolResult {
    let (ref_str, new_body) = edit_ctx;
    let event_id = resolve_event_ref(state, room_id, ref_str);
    let Some(event_id) = event_id else {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Cannot resolve edit ref '{ref_str}'."),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
        };
    };
    match client::send::edit_message(room_id, &event_id, new_body) {
        Ok(new_event_id) => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Message {ref_str} edited (new event: {new_event_id})."),
            display: None,
            is_error: false,
            tool_name: tool.name.clone(),
        },
        Err(e) => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Edit failed: {e}"),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
        },
    }
}

/// `Chat_react` — send a reaction emoji on a message.
fn execute_react(tool: &ToolUse, state: &State) -> ToolResult {
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
            };
        }
    };

    // Resolve short ref (E3) to full event ID
    let event_id = resolve_event_ref(state, &room_id, event_ref);
    let Some(event_id) = event_id else {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Cannot resolve event ref '{event_ref}'."),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
        };
    };

    match client::send::send_reaction(&room_id, &event_id, emoji) {
        Ok(_reaction_event_id) => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Reacted {emoji} to {event_ref} in '{room_input}'."),
            display: None,
            is_error: false,
            tool_name: tool.name.clone(),
        },
        Err(e) => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Reaction failed: {e}"),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
        },
    }
}

/// `Chat_configure` — update the room panel's filter settings.
///
/// All params optional. Omitted params keep current value.
/// Call with no filter params to reset to defaults.
fn execute_configure(tool: &ToolUse, state: &mut State) -> ToolResult {
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
        };
    };

    if !has_any_param {
        // Reset to defaults — clear all winds, return to calm seas
        open.filter = crate::types::RoomFilter::default();
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Filters reset to defaults for '{room_input}'."),
            display: None,
            is_error: false,
            tool_name: tool.name.clone(),
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
    // Trim trailing ", "
    if summary.ends_with(", ") {
        summary.truncate(summary.len().saturating_sub(2));
    }

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: summary,
        display: None,
        is_error: false,
        tool_name: tool.name.clone(),
    }
}
