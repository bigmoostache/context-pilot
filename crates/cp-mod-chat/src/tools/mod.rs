//! Tool dispatch for all `Chat_*` tools.
//!
//! Each tool is routed to its implementation sub-module. During the scaffold
//! phase (§1–§2), tools that depend on the sync loop return placeholder
//! results while server lifecycle tools are functional.

use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::server;
use crate::types::{ChatState, ServerStatus};

/// Route a `Chat_*` tool call to the appropriate handler.
pub(crate) fn dispatch(tool: &ToolUse, state: &mut State) -> ToolResult {
    match tool.name.as_str() {
        "Chat_open" => execute_open(tool, state),
        "Chat_send" => stub(tool, "Chat_send: not yet implemented (§6)"),
        "Chat_react" => stub(tool, "Chat_react: not yet implemented (§6)"),
        "Chat_configure" => stub(tool, "Chat_configure: not yet implemented (§6)"),
        "Chat_search" => stub(tool, "Chat_search: not yet implemented (§7)"),
        "Chat_mark_as_read" => stub(tool, "Chat_mark_as_read: not yet implemented (§7)"),
        "Chat_create_room" => stub(tool, "Chat_create_room: not yet implemented (§7)"),
        "Chat_invite" => stub(tool, "Chat_invite: not yet implemented (§7)"),
        _ => stub(tool, "Unknown chat tool"),
    }
}

/// Placeholder result for unimplemented tools.
fn stub(tool: &ToolUse, msg: &str) -> ToolResult {
    ToolResult { tool_use_id: tool.id.clone(), content: msg.to_string(), is_error: true, tool_name: tool.name.clone() }
}

/// `Chat_open` — ensures the server is running, then opens a room panel.
///
/// During §2, this verifies the server lifecycle. Full room opening
/// comes in §5–§6 when the sync loop is implemented.
fn execute_open(tool: &ToolUse, state: &mut State) -> ToolResult {
    let cs = ChatState::get(state);

    // If server not running, attempt to start it
    if cs.server_status != ServerStatus::Running
        && let Err(e) = server::start_server(state)
    {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Cannot start chat server: {e}"),
            is_error: true,
            tool_name: tool.name.clone(),
        };
    }

    // Server running — room opening will be implemented in §5-§6
    let room = tool.input.get("room").and_then(serde_json::Value::as_str).unwrap_or("#general");

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Server running. Room panel for '{room}' not yet implemented (§5-§6)."),
        is_error: false,
        tool_name: tool.name.clone(),
    }
}
