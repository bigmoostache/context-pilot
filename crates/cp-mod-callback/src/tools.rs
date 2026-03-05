use std::path::PathBuf;

use cp_base::config::constants;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::tools_upsert;
use crate::types::CallbackState;

/// Execute the `Callback_upsert` tool (create/update/delete callbacks).
pub fn execute_upsert(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(action) = tool.input.get("action").and_then(|v| v.as_str()) else {
        return ToolResult::new(
            tool.id.clone(),
            "Missing required parameter 'action' (create/update/delete)".to_string(),
            true,
        );
    };

    match action {
        "create" => tools_upsert::execute_create(tool, state),
        "update" => tools_upsert::execute_update(tool, state),
        "delete" => tools_upsert::execute_delete(tool, state),
        _ => ToolResult::new(
            tool.id.clone(),
            format!("Invalid action '{action}'. Use 'create', 'update', or 'delete'."),
            true,
        ),
    }
}

/// Open a callback's script in the panel editor for viewing/editing.
pub fn execute_open_editor(tool: &ToolUse, state: &mut State) -> ToolResult {
    let anchor_id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => {
            return ToolResult::new(tool.id.clone(), "Missing required parameter 'id'".to_string(), true);
        }
    };

    let cs = CallbackState::get(state);
    let Some(def) = cs.definitions.iter().find(|d| d.id == anchor_id) else {
        return ToolResult::new(tool.id.clone(), format!("Callback '{anchor_id}' not found"), true);
    };

    // Read the script file so we can confirm it exists
    let script_path = PathBuf::from(constants::STORE_DIR).join("scripts").join(format!("{}.sh", def.name));
    if !script_path.exists() {
        return ToolResult::new(
            tool.id.clone(),
            format!(
                "Script file not found: .context-pilot/scripts/{}.sh — the callback definition exists but the script is missing.",
                def.name
            ),
            true,
        );
    }

    let previous = CallbackState::get(state).editor_open.clone();
    CallbackState::get_mut(state).editor_open = Some(anchor_id.clone());

    // Touch the callback panel to trigger re-render with editor content
    for ctx in &mut state.context {
        if ctx.context_type.as_str() == cp_base::state::context::Kind::CALLBACK {
            ctx.last_refresh_ms = 0; // Force refresh
            break;
        }
    }

    let msg = previous.as_ref().map_or_else(
        || format!("Opened callback {anchor_id} in editor. Script content is now visible in the Callbacks panel."),
        |prev| format!("Opened callback {anchor_id} in editor (closed previous: {prev}). Script content is now visible in the Callbacks panel."),
    );

    ToolResult::new(tool.id.clone(), msg, false)
}

/// Close the callback editor, restoring the normal table view.
pub fn execute_close_editor(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(previous) = CallbackState::get(state).editor_open.clone() else {
        return ToolResult::new(tool.id.clone(), "No callback editor is currently open.".to_string(), true);
    };

    CallbackState::get_mut(state).editor_open = None;

    // Touch the callback panel to trigger re-render
    for ctx in &mut state.context {
        if ctx.context_type.as_str() == cp_base::state::context::Kind::CALLBACK {
            ctx.last_refresh_ms = 0;
            break;
        }
    }

    ToolResult::new(
        tool.id.clone(),
        format!("Closed callback editor (was viewing '{previous}'). Callbacks panel restored to table view."),
        false,
    )
}

/// Execute the `Callback_toggle` tool (activate/deactivate per worker).
pub fn execute_toggle(tool: &ToolUse, state: &mut State) -> ToolResult {
    let anchor_id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => {
            return ToolResult::new(tool.id.clone(), "Missing required parameter 'id'".to_string(), true);
        }
    };

    let Some(active) = tool.input.get("active").and_then(serde_json::Value::as_bool) else {
        return ToolResult::new(tool.id.clone(), "Missing required parameter 'active' (true/false)".to_string(), true);
    };

    let cs = CallbackState::get(state);
    if !cs.definitions.iter().any(|d| d.id == anchor_id) {
        return ToolResult::new(tool.id.clone(), format!("Callback '{anchor_id}' not found"), true);
    }

    let cs_mut = CallbackState::get_mut(state);
    if active {
        let _ = cs_mut.active_set.insert(anchor_id.clone());
        ToolResult::new(tool.id.clone(), format!("Callback {anchor_id} activated ✓"), false)
    } else {
        let _ = cs_mut.active_set.remove(&anchor_id);
        ToolResult::new(tool.id.clone(), format!("Callback {anchor_id} deactivated ✗"), false)
    }
}
