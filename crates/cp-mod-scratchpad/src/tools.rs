use cp_base::state::{ContextType, ScratchpadCell, State};
use cp_base::tools::{ToolResult, ToolUse};

/// Create a new scratchpad cell
pub fn execute_create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let title = match tool.input.get("cell_title").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'cell_title' parameter".to_string(),
                is_error: true,
            };
        }
    };

    let contents = match tool.input.get("cell_contents").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'cell_contents' parameter".to_string(),
                is_error: true,
            };
        }
    };

    let id = format!("C{}", state.next_scratchpad_id);
    state.next_scratchpad_id += 1;

    state.scratchpad_cells.push(ScratchpadCell { id: id.clone(), title: title.clone(), content: contents.clone() });

    // Update Scratchpad panel timestamp
    state.touch_panel(ContextType::new(ContextType::SCRATCHPAD));

    let preview =
        if contents.len() > 50 { format!("{}...", &contents[..contents.floor_char_boundary(47)]) } else { contents };

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Created cell {} '{}': {}", id, title, preview),
        is_error: false,
    }
}

/// Edit an existing scratchpad cell
pub fn execute_edit(tool: &ToolUse, state: &mut State) -> ToolResult {
    let cell_id = match tool.input.get("cell_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'cell_id' parameter".to_string(),
                is_error: true,
            };
        }
    };

    let cell = state.scratchpad_cells.iter_mut().find(|c| c.id == cell_id);

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
                ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("No changes specified for cell {}", cell_id),
                    is_error: true,
                }
            } else {
                // Update Scratchpad panel timestamp
                state.touch_panel(ContextType::new(ContextType::SCRATCHPAD));
                ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Updated cell {}: {}", cell_id, changes.join(", ")),
                    is_error: false,
                }
            }
        }
        None => {
            ToolResult { tool_use_id: tool.id.clone(), content: format!("Cell not found: {}", cell_id), is_error: true }
        }
    }
}

/// Wipe scratchpad cells (delete by IDs, or all if empty array)
pub fn execute_wipe(tool: &ToolUse, state: &mut State) -> ToolResult {
    let cell_ids = match tool.input.get("cell_ids").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'cell_ids' array parameter".to_string(),
                is_error: true,
            };
        }
    };

    // If empty array, wipe all cells
    if cell_ids.is_empty() {
        let count = state.scratchpad_cells.len();
        state.scratchpad_cells.clear();
        // Update Scratchpad panel timestamp
        state.touch_panel(ContextType::new(ContextType::SCRATCHPAD));
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Wiped all {} scratchpad cell(s)", count),
            is_error: false,
        };
    }

    // Otherwise, delete specific cells
    let ids_to_delete: Vec<String> = cell_ids.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();

    let initial_count = state.scratchpad_cells.len();
    state.scratchpad_cells.retain(|c| !ids_to_delete.contains(&c.id));
    let deleted_count = initial_count - state.scratchpad_cells.len();

    let mut output = format!("Deleted {} cell(s)", deleted_count);

    if deleted_count < ids_to_delete.len() {
        let missing_count = ids_to_delete.len() - deleted_count;
        output.push_str(&format!(", {} not found", missing_count));
    }

    // Update Scratchpad panel timestamp if any cells were deleted
    if deleted_count > 0 {
        state.touch_panel(ContextType::new(ContextType::SCRATCHPAD));
    }

    ToolResult { tool_use_id: tool.id.clone(), content: output, is_error: deleted_count == 0 }
}
