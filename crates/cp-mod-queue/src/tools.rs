use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::QueueState;
use std::fmt::Write as _;

/// Execute `Queue_pause`: stop intercepting, tools execute normally. Queue stays intact.
pub(crate) fn execute_pause(tool: &ToolUse, state: &mut State) -> ToolResult {
    let qs = QueueState::get_mut(state);
    if !qs.active {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "Queue is already paused/inactive.".to_string(),
            display: None,
            is_error: false,
            tool_name: tool.name.clone(),
        };
    }
    qs.active = false;
    let n = qs.queued_calls.len();
    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Queue paused. Tools now execute normally. {n} action(s) still queued."),
        display: None,
        is_error: false,
        tool_name: tool.name.clone(),
    }
}

/// Execute `Queue_undo`: remove specific queued action(s) by index.
pub(crate) fn execute_undo(tool: &ToolUse, state: &mut State) -> ToolResult {
    let indices: Vec<usize> = match tool.input.get("indices").and_then(|v| v.as_array()) {
        Some(arr) => arr.iter().filter_map(|v| v.as_u64().map(cp_base::cast::Safe::to_usize)).collect(),
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'indices' parameter (expected array of numbers).".to_string(),
                display: None,
                is_error: true,
                tool_name: tool.name.clone(),
            };
        }
    };

    let qs = QueueState::get_mut(state);
    let mut removed = Vec::new();
    let mut not_found = Vec::new();
    for idx in indices {
        if qs.remove_by_index(idx) {
            removed.push(idx.to_string());
        } else {
            not_found.push(idx.to_string());
        }
    }

    let mut msg = String::new();
    if !removed.is_empty() {
        let _r = write!(msg, "Removed: #{}", removed.join(", #"));
    }
    if !not_found.is_empty() {
        if !msg.is_empty() {
            msg.push_str(". ");
        }
        let _r = write!(msg, "Not found: #{}", not_found.join(", #"));
    }
    let _r = write!(msg, ". {} action(s) remaining.", qs.queued_calls.len());

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: msg,
        display: None,
        is_error: !not_found.is_empty() && removed.is_empty(),
        tool_name: tool.name.clone(),
    }
}

/// Execute `Queue_empty`: discard all queued actions without executing.
pub(crate) fn execute_empty(tool: &ToolUse, state: &mut State) -> ToolResult {
    let qs = QueueState::get_mut(state);
    let n = qs.queued_calls.len();
    qs.clear();
    qs.active = false;
    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Queue emptied. Discarded {n} action(s)."),
        display: None,
        is_error: false,
        tool_name: tool.name.clone(),
    }
}
