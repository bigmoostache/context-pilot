use crate::storage;
use crate::types::{PromptItem, PromptState, PromptType};
use cp_base::config::library;
use cp_base::state::{ContextType, State};
use cp_base::tools::{ToolResult, ToolUse};

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
        return ToolResult::new(tool.id.clone(), format!("Agent with ID '{}' already exists", id), true);
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

    state.touch_panel(ContextType::new(ContextType::SYSTEM));
    state.touch_panel(ContextType::new(ContextType::LIBRARY));

    ToolResult::new(tool.id.clone(), format!("Created agent '{}' with ID '{}'", name, id), false)
}

pub(crate) fn delete(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(id) = tool.input.get("id").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing required 'id' parameter".to_string(), true);
    };

    // Cannot delete built-in agents
    if let Some(agent) = PromptState::get(state).agents.iter().find(|a| a.id == id)
        && agent.is_builtin
    {
        return ToolResult::new(tool.id.clone(), format!("Cannot delete built-in agent '{}'", id), true);
    }

    let ps = PromptState::get_mut(state);
    let Some(idx) = ps.agents.iter().position(|a| a.id == id) else {
        return ToolResult::new(tool.id.clone(), format!("Agent '{}' not found", id), true);
    };

    let agent = ps.agents.remove(idx);
    storage::delete_prompt_from_dir(&storage::dir_for(PromptType::Agent), id);

    // If this was the active agent, switch to default
    if ps.active_agent_id.as_deref() == Some(id) {
        ps.active_agent_id = Some(library::default_agent_id().to_string());
    }

    state.touch_panel(ContextType::new(ContextType::SYSTEM));
    state.touch_panel(ContextType::new(ContextType::LIBRARY));

    ToolResult::new(tool.id.clone(), format!("Deleted agent '{}' ({})", agent.name, id), false)
}

pub(crate) fn load(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id = tool.input.get("id").and_then(|v| v.as_str());

    // If id is None or empty, switch to default agent
    if id.is_none() || id.map(|s| s.is_empty()).unwrap_or(true) {
        PromptState::get_mut(state).active_agent_id = Some(library::default_agent_id().to_string());
        state.touch_panel(ContextType::new(ContextType::SYSTEM));
        state.touch_panel(ContextType::new(ContextType::LIBRARY));
        return ToolResult::new(
            tool.id.clone(),
            format!("Switched to default agent ({})", library::default_agent_id()),
            false,
        );
    }

    let id = id.unwrap();

    if !PromptState::get(state).agents.iter().any(|a| a.id == id) {
        return ToolResult::new(tool.id.clone(), format!("Agent '{}' not found", id), true);
    }

    PromptState::get_mut(state).active_agent_id = Some(id.to_string());
    state.touch_panel(ContextType::new(ContextType::SYSTEM));
    state.touch_panel(ContextType::new(ContextType::LIBRARY));

    let name = PromptState::get(state).agents.iter().find(|a| a.id == id).map(|a| a.name.as_str()).unwrap_or("unknown");

    ToolResult::new(tool.id.clone(), format!("Loaded agent '{}' ({})", name, id), false)
}
