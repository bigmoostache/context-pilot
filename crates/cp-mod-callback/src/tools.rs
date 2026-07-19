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
            "Missing required parameter 'action' (create/update/delete)".to_owned(),
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
    let key = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_owned(),
        None => {
            return ToolResult::new(tool.id.clone(), "Missing required parameter 'id'".to_owned(), true);
        }
    };

    let cs = CallbackState::get(state);
    let Some(def) = cs.find_by_name_or_id(&key) else {
        return ToolResult::new(tool.id.clone(), format!("Callback '{key}' not found"), true);
    };
    let def_name = def.name.clone();
    let def_id = def.id.clone();

    // Read the script file so we can confirm it exists
    let script_path = PathBuf::from(constants::STORE_DIR).join("scripts").join(format!("{def_name}.sh"));
    if !script_path.exists() {
        return ToolResult::new(
            tool.id.clone(),
            format!(
                "Script file not found: .context-pilot/scripts/{def_name}.sh — the callback definition exists but the script is missing.",
            ),
            true,
        );
    }

    let previous = CallbackState::get(state).editor_open.clone();
    CallbackState::get_mut(state).editor_open = Some(def_name.clone());

    // Touch the callback panel to trigger re-render with editor content
    for ctx in &mut state.context {
        if ctx.context_type.as_str() == cp_base::state::context::Kind::CALLBACK {
            ctx.last_refresh_ms = 0; // Force refresh
            break;
        }
    }

    let msg = previous.as_ref().map_or_else(
        || format!("Opened callback {def_id} [{def_name}] in editor. Script content is now visible in the Callbacks panel."),
        |prev| format!("Opened callback {def_id} [{def_name}] in editor (closed previous: {prev}). Script content is now visible in the Callbacks panel."),
    );

    ToolResult::new(tool.id.clone(), msg, false)
}

/// Close the callback editor, restoring the normal table view.
pub fn execute_close_editor(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(previous) = CallbackState::get(state).editor_open.clone() else {
        return ToolResult::new(tool.id.clone(), "No callback editor is currently open.".to_owned(), true);
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
