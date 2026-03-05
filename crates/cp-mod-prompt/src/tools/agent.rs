use crate::storage;
use crate::types::{PromptItem, PromptState, PromptType};
use cp_base::config::accessors::library;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

/// Create a new agent from tool parameters and persist it.
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

    if PromptState::get(state).agents.iter().any(|a| a.id == id) {
        return ToolResult::new(tool.id.clone(), format!("Agent with ID '{id}' already exists"), true);
    }

    let item = PromptItem {
        id: id.clone(),
        name: name.clone(),
        description,
        content,
        prompt_type: PromptType::Agent,
        is_builtin: false,
    };

    storage::save_prompt_to_dir(&storage::dir_for(PromptType::Agent), &item);
    PromptState::get_mut(state).agents.push(item);

    state.touch_panel(Kind::SYSTEM);
    state.touch_panel(Kind::LIBRARY);

    ToolResult::new(tool.id.clone(), format!("Created agent '{name}' with ID '{id}'"), false)
}

/// Delete an agent by ID, falling back to the default agent if active.
pub(crate) fn delete(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(id) = tool.input.get("id").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing required 'id' parameter".to_string(), true);
    };

    // Cannot delete built-in agents
    if let Some(agent) = PromptState::get(state).agents.iter().find(|a| a.id == id)
        && agent.is_builtin
    {
        return ToolResult::new(tool.id.clone(), format!("Cannot delete built-in agent '{id}'"), true);
    }

    let ps = PromptState::get_mut(state);
    let Some(idx) = ps.agents.iter().position(|a| a.id == id) else {
        return ToolResult::new(tool.id.clone(), format!("Agent '{id}' not found"), true);
    };

    let agent = ps.agents.remove(idx);
    storage::delete_prompt_from_dir(&storage::dir_for(PromptType::Agent), id);

    // If this was the active agent, switch to default
    if ps.active_agent_id.as_deref() == Some(id) {
        ps.active_agent_id = Some(library::default_agent_id().to_string());
    }

    state.touch_panel(Kind::SYSTEM);
    state.touch_panel(Kind::LIBRARY);

    ToolResult::new(tool.id.clone(), format!("Deleted agent '{}' ({})", agent.name, id), false)
}

/// Set the active agent by ID, or revert to the default agent.
pub(crate) fn load(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id = tool.input.get("id").and_then(|v| v.as_str());

    // If id is None or empty, switch to default agent
    let Some(id) = id.filter(|s| !s.is_empty()) else {
        PromptState::get_mut(state).active_agent_id = Some(library::default_agent_id().to_string());
        state.touch_panel(Kind::SYSTEM);
        state.touch_panel(Kind::LIBRARY);
        return ToolResult::new(
            tool.id.clone(),
            format!("Switched to default agent ({})", library::default_agent_id()),
            false,
        );
    };

    if !PromptState::get(state).agents.iter().any(|a| a.id == id) {
        return ToolResult::new(tool.id.clone(), format!("Agent '{id}' not found"), true);
    }

    PromptState::get_mut(state).active_agent_id = Some(id.to_string());
    state.touch_panel(Kind::SYSTEM);
    state.touch_panel(Kind::LIBRARY);

    let name = PromptState::get(state).agents.iter().find(|a| a.id == id).map_or("unknown", |a| a.name.as_str());

    ToolResult::new(tool.id.clone(), format!("Loaded agent '{name}' ({id})"), false)
}
