use crate::storage;
use crate::types::{PromptItem, PromptState, PromptType};
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

/// Create a new slash-command from tool parameters and persist it.
pub(crate) fn create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let name = tool.input.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let description = tool.input.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let content = tool.input.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();

    if name.is_empty() {
        return ToolResult::new(tool.id.clone(), "Missing required 'name' parameter".to_string(), true);
    }

    if content.is_empty() {
        return ToolResult::new(tool.id.clone(), "Missing required 'content' parameter".to_string(), true);
    }

    let id = storage::slugify(&name);
    if id.is_empty() {
        return ToolResult::new(
            tool.id.clone(),
            "Name must contain at least one alphanumeric character".to_string(),
            true,
        );
    }

    if PromptState::get(state).commands.iter().any(|c| c.id == id) {
        return ToolResult::new(tool.id.clone(), format!("Command with ID '{id}' already exists"), true);
    }

    let item = PromptItem {
        id: id.clone(),
        name: name.clone(),
        description,
        content,
        prompt_type: PromptType::Command,
        is_builtin: false,
    };

    storage::save_prompt_to_dir(&storage::dir_for(PromptType::Command), &item);
    PromptState::get_mut(state).commands.push(item);

    state.touch_panel(Kind::LIBRARY);

    ToolResult::new(tool.id.clone(), format!("Created command '{name}' with ID '{id}' (use as /{id})"), false)
}

/// Delete a command by ID and remove it from storage.
pub(crate) fn delete(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(id) = tool.input.get("id").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing required 'id' parameter".to_string(), true);
    };

    if let Some(cmd) = PromptState::get(state).commands.iter().find(|c| c.id == id)
        && cmd.is_builtin
    {
        return ToolResult::new(tool.id.clone(), format!("Cannot delete built-in command '{id}'"), true);
    }

    let ps = PromptState::get_mut(state);
    let Some(idx) = ps.commands.iter().position(|c| c.id == id) else {
        return ToolResult::new(tool.id.clone(), format!("Command '{id}' not found"), true);
    };

    let cmd = ps.commands.remove(idx);
    storage::delete_prompt_from_dir(&storage::dir_for(PromptType::Command), id);

    state.touch_panel(Kind::LIBRARY);

    ToolResult::new(tool.id.clone(), format!("Deleted command '{}' ({})", cmd.name, id), false)
}
