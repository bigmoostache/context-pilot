use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::{MAX_CELL_BYTES, MAX_CELLS_PER_AGENT, ScratchpadCell, ScratchpadState};
use std::fmt::Write as _;

/// Error text when no agent is executing (cells always belong to one).
const NO_AGENT: &str = "No focused agent \u{2014} scratchpad cells live in an agent; Read an agent first.";

/// Record the focused agent id; `true` when it changed.
///
/// Scoping does NOT read this value (it follows `State::executing_agent_id`);
/// the main crate only uses the return value to force a Scratchpad panel
/// refresh on a focus switch. Mirrors `cp_mod_todo::tools::set_focus_filter`.
pub fn set_focus_filter(state: &mut State, agent_id: Option<String>) -> bool {
    // Read first: `get_mut` refreshes the change stamp, and this runs every tick.
    if ScratchpadState::get(state).focus_filter == agent_id {
        return false;
    }
    ScratchpadState::get_mut(state).focus_filter = agent_id;
    true
}

/// Drop every cell lacking an `agent_id` (the legacy, pre-rework backlog).
/// Called once on load — a permanent purge (mirrors todo FR4).
pub fn purge_agentless(state: &mut State) {
    if ScratchpadState::get(state).scratchpad_cells.iter().all(|c| !c.agent_id.is_empty()) {
        return;
    }
    ScratchpadState::get_mut(state).scratchpad_cells.retain(|c| !c.agent_id.is_empty());
}

/// Remove every cell and the id counter owned by `agent_id` — cascade
/// cleanup when an agent is hard-deleted. Returns the number of cells removed.
pub fn purge_agent_cells(state: &mut State, agent_id: &str) -> usize {
    let ro = ScratchpadState::get(state);
    if ro.agent_cell_count(agent_id) == 0 && !ro.next_ids.contains_key(agent_id) {
        return 0;
    }
    let ss = ScratchpadState::get_mut(state);
    let _counter = ss.next_ids.remove(agent_id);
    let before = ss.scratchpad_cells.len();
    ss.scratchpad_cells.retain(|c| c.agent_id != agent_id);
    before.saturating_sub(ss.scratchpad_cells.len())
}

/// Error text if `title + content` exceeds [`MAX_CELL_BYTES`].
fn oversize(title: &str, content: &str) -> Option<String> {
    let size = title.len().saturating_add(content.len());
    (size > MAX_CELL_BYTES).then(|| {
        format!("Cell too large: {size} bytes (max {MAX_CELL_BYTES}). Keep scratchpad cells short; split or summarize.")
    })
}

/// Create a new scratchpad cell
pub(crate) fn execute_create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("scratch_create");
    let err = |msg: String| ToolResult::new(tool.id.clone(), msg, true);
    let Some(title) = tool.input.get("cell_title").and_then(|v| v.as_str()) else {
        return err("Missing 'cell_title' parameter".to_owned());
    };
    let Some(contents) = tool.input.get("cell_contents").and_then(|v| v.as_str()) else {
        return err("Missing 'cell_contents' parameter".to_owned());
    };
    let Some(agent_id) = state.executing_agent_id().map(str::to_owned) else {
        return err(NO_AGENT.to_owned());
    };
    if let Some(msg) = oversize(title, contents) {
        return err(msg);
    }
    if ScratchpadState::get(state).agent_cell_count(&agent_id) >= MAX_CELLS_PER_AGENT {
        return err(format!("Too many cells (max {MAX_CELLS_PER_AGENT}). Wipe cells you no longer need first."));
    }

    let ss = ScratchpadState::get_mut(state);
    let id = ss.alloc_id(&agent_id);
    ss.scratchpad_cells.push(ScratchpadCell {
        id: id.clone(),
        agent_id,
        title: title.to_owned(),
        content: contents.to_owned(),
    });
    state.touch_panel(Kind::SCRATCHPAD);

    let preview = if contents.len() > 50 {
        format!("{}...", contents.get(..contents.floor_char_boundary(47)).unwrap_or(""))
    } else {
        contents.to_owned()
    };
    ToolResult::new(tool.id.clone(), format!("Created cell {id} '{title}': {preview}"), false)
}

/// Edit an existing scratchpad cell
pub(crate) fn execute_edit(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("scratch_edit");
    let err = |msg: String| ToolResult::new(tool.id.clone(), msg, true);
    let Some(cell_id) = tool.input.get("cell_id").and_then(|v| v.as_str()) else {
        return err("Missing 'cell_id' parameter".to_owned());
    };
    let Some(agent_id) = state.executing_agent_id().map(str::to_owned) else {
        return err(NO_AGENT.to_owned());
    };
    let new_title = tool.input.get("cell_title").and_then(|v| v.as_str());
    let new_content = tool.input.get("cell_contents").and_then(|v| v.as_str());

    // Validate against a shared borrow: a rejected edit must not bump the stamp.
    let Some(cell) = ScratchpadState::get(state).agent_cells(&agent_id).find(|c| c.id == cell_id) else {
        return err(format!("Cell not found: {cell_id}"));
    };
    if new_title.is_none() && new_content.is_none() {
        return err(format!("No changes specified for cell {cell_id}"));
    }
    if let Some(msg) = oversize(new_title.unwrap_or(&cell.title), new_content.unwrap_or(&cell.content)) {
        return err(msg);
    }

    let mut changes = Vec::new();
    let ss = ScratchpadState::get_mut(state);
    if let Some(c) = ss.scratchpad_cells.iter_mut().find(|c| c.id == cell_id && c.agent_id == agent_id) {
        if let Some(t) = new_title {
            t.clone_into(&mut c.title);
            changes.push("title");
        }
        if let Some(body) = new_content {
            body.clone_into(&mut c.content);
            changes.push("contents");
        }
    }
    state.touch_panel(Kind::SCRATCHPAD);
    ToolResult::new(tool.id.clone(), format!("Updated cell {}: {}", cell_id, changes.join(", ")), false)
}

/// Wipe scratchpad cells (delete by IDs, or all if empty array)
pub(crate) fn execute_wipe(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("scratch_wipe");
    let Some(cell_ids) = tool.input.get("cell_ids").and_then(|v| v.as_array()) else {
        return ToolResult::new(tool.id.clone(), "Missing 'cell_ids' array parameter".to_owned(), true);
    };
    let Some(agent_id) = state.executing_agent_id().map(str::to_owned) else {
        return ToolResult::new(tool.id.clone(), NO_AGENT.to_owned(), true);
    };

    // Empty array = all of THIS agent's cells.
    let wipe_all = cell_ids.is_empty();
    let ids_to_delete: Vec<String> = cell_ids.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect();
    let doomed = |c: &ScratchpadCell| c.agent_id == agent_id && (wipe_all || ids_to_delete.contains(&c.id));

    // Count first: a wipe that matches nothing must not bump the change stamp.
    let deleted_count = ScratchpadState::get(state).scratchpad_cells.iter().filter(|c| doomed(c)).count();
    if deleted_count > 0 {
        ScratchpadState::get_mut(state).scratchpad_cells.retain(|c| !doomed(c));
        state.touch_panel(Kind::SCRATCHPAD);
    }

    if wipe_all {
        return ToolResult::new(tool.id.clone(), format!("Wiped all {deleted_count} scratchpad cell(s)"), false);
    }
    let mut output = format!("Deleted {deleted_count} cell(s)");
    let missing_count = ids_to_delete.len().saturating_sub(deleted_count);
    if missing_count > 0 {
        let _r = write!(output, ", {missing_count} not found");
    }
    // Missing ids are only a pre_flight warning, so nothing-deleted is not an error.
    ToolResult::new(tool.id.clone(), output, false)
}
