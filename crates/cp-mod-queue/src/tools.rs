use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::QueueState;
use std::fmt::Write as _;

/// Execute `Queue_pause`: stop intercepting, tools execute normally. Queue stays intact.
pub(crate) fn execute_pause(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("queue_pause");
    let qs = QueueState::get_mut(state);
    if !qs.active {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "Queue is already paused/inactive.".to_owned(),
            display: None,
            tldr: None,
            is_error: false,
            preserves_tempo: false,
            tool_name: tool.name.clone(),
        };
    }
    qs.active = false;
    let n = qs.queued_calls.len();
    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Queue paused. Tools now execute normally. {n} action(s) still queued."),
        display: None,
        tldr: None,
        is_error: false,
        preserves_tempo: true,
        tool_name: tool.name.clone(),
    }
}

/// Format the undo result line from removed/not-found indices and queue state.
fn format_undo_result(removed: &[String], not_found: &[String], remaining: usize, deactivated: bool) -> String {
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
    if deactivated {
        let _r = write!(msg, ". Queue empty \u{2014} deactivated.");
    } else {
        let _r = write!(msg, ". {remaining} action(s) remaining.");
    }
    msg
}

/// Execute `Queue_undo`: remove specific queued action(s) by index.
pub(crate) fn execute_undo(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("queue_undo");
    let indices: Vec<usize> = match tool.input.get("indices").and_then(|v| v.as_array()) {
        Some(arr) => arr.iter().filter_map(|v| v.as_u64().map(cp_base::cast::Safe::to_usize)).collect(),
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'indices' parameter (expected array of numbers).".to_owned(),
                display: None,
                tldr: None,
                is_error: true,
                preserves_tempo: false,
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

    // Auto-deactivate when the queue is drained completely
    let deactivated = qs.queued_calls.is_empty() && qs.active;
    if deactivated {
        qs.active = false;
        qs.next_index = 1;
    }
    let msg = format_undo_result(&removed, &not_found, qs.queued_calls.len(), deactivated);

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: msg,
        display: None,
        tldr: None,
        is_error: !not_found.is_empty() && removed.is_empty(),
        preserves_tempo: false,
        tool_name: tool.name.clone(),
    }
}
