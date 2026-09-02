use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::{ScratchpadCell, ScratchpadState};
use std::fmt::Write as _;

/// Set the injected focused-thread filter used by the panel + tools. Returns
/// whether it changed (which drives the caller's forced panel refresh).
/// Mirrors `cp_mod_todo::tools::set_focus_filter`.
pub fn set_focus_filter(state: &mut State, thread_id: Option<String>) -> bool {
    let ss = ScratchpadState::get_mut(state);
    if ss.focus_filter == thread_id {
        false
    } else {
        ss.focus_filter = thread_id;
        true
    }
}

/// Drop every cell lacking a `thread_id` (the legacy, pre-rework backlog).
/// Called once on load — a permanent, forever purge (mirrors todo FR4).
pub fn purge_threadless(state: &mut State) {
    ScratchpadState::get_mut(state).scratchpad_cells.retain(|c| !c.thread_id.is_empty());
}

/// Remove every cell owned by `thread_id` — cascade cleanup when a thread is
/// hard-deleted (mirrors the thread-owned model). Returns the number removed.
pub fn purge_thread_cells(state: &mut State, thread_id: &str) -> usize {
    let ss = ScratchpadState::get_mut(state);
    let before = ss.scratchpad_cells.len();
    ss.scratchpad_cells.retain(|c| c.thread_id != thread_id);
    before.saturating_sub(ss.scratchpad_cells.len())
}

/// Create a new scratchpad cell
pub(crate) fn execute_create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("scratch_create");
    let title = match tool.input.get("cell_title").and_then(|v| v.as_str()) {
        Some(t) => t.to_owned(),
        None => {
            return ToolResult::new(tool.id.clone(), "Missing 'cell_title' parameter".to_owned(), true);
        }
    };

    let contents = match tool.input.get("cell_contents").and_then(|v| v.as_str()) {
        Some(c) => c.to_owned(),
        None => {
            return ToolResult::new(tool.id.clone(), "Missing 'cell_contents' parameter".to_owned(), true);
        }
    };

    // Thread-owned: a cell must live in the focused thread (mirrors Think.todo).
    let Some(thread_id) = ScratchpadState::get(state).focus_filter.clone() else {
        return ToolResult::new(
            tool.id.clone(),
            "No focused thread \u{2014} scratchpad cells live in a thread; Read a thread first.".to_owned(),
            true,
        );
    };

    let ss = ScratchpadState::get_mut(state);
    let id = format!("C{}", ss.next_scratchpad_id);
    ss.next_scratchpad_id = ss.next_scratchpad_id.saturating_add(1);
    ss.scratchpad_cells.push(ScratchpadCell {
        id: id.clone(),
        thread_id,
        title: title.clone(),
        content: contents.clone(),
    });

    // Update Scratchpad panel timestamp
    state.touch_panel(Kind::SCRATCHPAD);

    let preview = if contents.len() > 50 {
        format!("{}...", contents.get(..contents.floor_char_boundary(47)).unwrap_or(""))
    } else {
        contents
    };

    ToolResult::new(tool.id.clone(), format!("Created cell {id} '{title}': {preview}"), false)
}

/// Edit an existing scratchpad cell
pub(crate) fn execute_edit(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("scratch_edit");
    let Some(cell_id) = tool.input.get("cell_id").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing 'cell_id' parameter".to_owned(), true);
    };

    // Thread-owned: only cells of the focused thread are editable.
    let Some(thread_id) = ScratchpadState::get(state).focus_filter.clone() else {
        return ToolResult::new(
            tool.id.clone(),
            "No focused thread \u{2014} scratchpad cells live in a thread; Read a thread first.".to_owned(),
            true,
        );
    };

    let ss = ScratchpadState::get_mut(state);
    let cell = ss.scratchpad_cells.iter_mut().find(|c| c.id == cell_id && c.thread_id == thread_id);

    match cell {
        Some(c) => {
            let mut changes = Vec::new();

            if let Some(title) = tool.input.get("cell_title").and_then(|v| v.as_str()) {
                title.clone_into(&mut c.title);
                changes.push("title");
            }

            if let Some(contents) = tool.input.get("cell_contents").and_then(|v| v.as_str()) {
                contents.clone_into(&mut c.content);
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
    let _fg = cp_base::flame!("scratch_wipe");
    let Some(cell_ids) = tool.input.get("cell_ids").and_then(|v| v.as_array()) else {
        return ToolResult::new(tool.id.clone(), "Missing 'cell_ids' array parameter".to_owned(), true);
    };

    // Thread-owned: wiping only ever touches the focused thread's cells.
    let Some(thread_id) = ScratchpadState::get(state).focus_filter.clone() else {
        return ToolResult::new(
            tool.id.clone(),
            "No focused thread \u{2014} scratchpad cells live in a thread; Read a thread first.".to_owned(),
            true,
        );
    };

    // If empty array, wipe all of THIS thread's cells
    if cell_ids.is_empty() {
        let ss = ScratchpadState::get_mut(state);
        let before = ss.scratchpad_cells.len();
        ss.scratchpad_cells.retain(|c| c.thread_id != thread_id);
        let count = before.saturating_sub(ss.scratchpad_cells.len());
        // Update Scratchpad panel timestamp
        state.touch_panel(Kind::SCRATCHPAD);
        return ToolResult::new(tool.id.clone(), format!("Wiped all {count} scratchpad cell(s)"), false);
    }

    // Otherwise, delete specific cells (restricted to the focused thread)
    let ids_to_delete: Vec<String> = cell_ids.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect();

    let ss = ScratchpadState::get_mut(state);
    let initial_count = ss.scratchpad_cells.len();
    ss.scratchpad_cells.retain(|c| !(c.thread_id == thread_id && ids_to_delete.contains(&c.id)));
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
