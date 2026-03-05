use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::{ScratchpadCell, ScratchpadState};
use std::fmt::Write as _;

/// Create a new scratchpad cell
pub(crate) fn execute_create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let title = match tool.input.get("cell_title").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => {
            return ToolResult::new(tool.id.clone(), "Missing 'cell_title' parameter".to_string(), true);
        }
    };

    let contents = match tool.input.get("cell_contents").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => {
            return ToolResult::new(tool.id.clone(), "Missing 'cell_contents' parameter".to_string(), true);
        }
    };

    let ss = ScratchpadState::get_mut(state);
    let id = format!("C{}", ss.next_scratchpad_id);
    ss.next_scratchpad_id = ss.next_scratchpad_id.saturating_add(1);
    ss.scratchpad_cells.push(ScratchpadCell { id: id.clone(), title: title.clone(), content: contents.clone() });

    // Update Scratchpad panel timestamp
    state.touch_panel(Kind::SCRATCHPAD);

    let preview = if contents.len() > 50 {
        format!("{}...", &contents.get(..contents.floor_char_boundary(47)).unwrap_or(""))
    } else {
        contents
    };

    ToolResult::new(tool.id.clone(), format!("Created cell {id} '{title}': {preview}"), false)
}

/// Edit an existing scratchpad cell
pub(crate) fn execute_edit(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(cell_id) = tool.input.get("cell_id").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing 'cell_id' parameter".to_string(), true);
    };

    let ss = ScratchpadState::get_mut(state);
    let cell = ss.scratchpad_cells.iter_mut().find(|c| c.id == cell_id);

    match cell {
        Some(c) => {
            let mut changes = Vec::new();

            if let Some(title) = tool.input.get("cell_title").and_then(|v| v.as_str()) {
                c.title = title.to_string();
                changes.push("title");
            }

            if let Some(contents) = tool.input.get("cell_contents").and_then(|v| v.as_str()) {
                c.content = contents.to_string();
                changes.push("contents");
            }

            if changes.is_empty() {
                ToolResult::new(tool.id.clone(), format!("No changes specified for cell {cell_id}"), true)
            } else {
                // Update Scratchpad panel timestamp
                state.touch_panel(Kind::SCRATCHPAD);
                ToolResult::new(tool.id.clone(), format!("Updated cell {}: {}", cell_id, changes.join(", ")), false)
            }
        }
        None => ToolResult::new(tool.id.clone(), format!("Cell not found: {cell_id}"), true),
    }
}

/// Wipe scratchpad cells (delete by IDs, or all if empty array)
pub(crate) fn execute_wipe(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(cell_ids) = tool.input.get("cell_ids").and_then(|v| v.as_array()) else {
        return ToolResult::new(tool.id.clone(), "Missing 'cell_ids' array parameter".to_string(), true);
    };

    // If empty array, wipe all cells
    if cell_ids.is_empty() {
        let ss = ScratchpadState::get_mut(state);
        let count = ss.scratchpad_cells.len();
        ss.scratchpad_cells.clear();
        // Update Scratchpad panel timestamp
        state.touch_panel(Kind::SCRATCHPAD);
        return ToolResult::new(tool.id.clone(), format!("Wiped all {count} scratchpad cell(s)"), false);
    }

    // Otherwise, delete specific cells
    let ids_to_delete: Vec<String> = cell_ids.iter().filter_map(|v| v.as_str().map(ToString::to_string)).collect();

    let ss = ScratchpadState::get_mut(state);
    let initial_count = ss.scratchpad_cells.len();
    ss.scratchpad_cells.retain(|c| !ids_to_delete.contains(&c.id));
    let deleted_count = initial_count.saturating_sub(ss.scratchpad_cells.len());

    let mut output = format!("Deleted {deleted_count} cell(s)");

    if deleted_count < ids_to_delete.len() {
        let missing_count = ids_to_delete.len().saturating_sub(deleted_count);
        let _r = write!(output, ", {missing_count} not found");
    }

    // Update Scratchpad panel timestamp if any cells were deleted
    if deleted_count > 0 {
        state.touch_panel(Kind::SCRATCHPAD);
    }

    ToolResult::new(tool.id.clone(), output, deleted_count == 0)
}
