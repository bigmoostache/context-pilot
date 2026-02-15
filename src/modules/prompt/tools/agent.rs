use crate::constants::library;
use crate::modules::prompt::storage;
use crate::modules::prompt::types::{PromptItem, PromptType};
use crate::state::{ContextType, State};
use crate::tools::{ToolResult, ToolUse};

pub fn create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let name = tool.input.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let description = tool.input.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let content = tool.input.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();

    if name.is_empty() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "Missing required 'name' parameter".to_string(),
            is_error: true,
        };
    }

    if content.is_empty() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "Missing required 'content' parameter".to_string(),
            is_error: true,
        };
    }

    let id = storage::slugify(&name);
    if id.is_empty() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "Name must contain at least one alphanumeric character".to_string(),
            is_error: true,
        };
    }

    if state.agents.iter().any(|a| a.id == id) {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Agent with ID '{}' already exists", id),
            is_error: true,
        };
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
    state.agents.push(item);

    state.touch_panel(ContextType::System);
    state.touch_panel(ContextType::Library);

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Created agent '{}' with ID '{}'", name, id),
        is_error: false,
    }
}

pub fn edit(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required 'id' parameter".to_string(),
                is_error: true,
            };
        }
    };

    let agent = match state.agents.iter_mut().find(|a| a.id == id) {
        Some(a) => a,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Agent '{}' not found", id),
                is_error: true,
            };
        }
    };

    if agent.is_builtin {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Cannot edit built-in agent '{}'", id),
            is_error: true,
        };
    }

    let mut changes = Vec::new();

    if let Some(name) = tool.input.get("name").and_then(|v| v.as_str()) {
        agent.name = name.to_string();
        changes.push("name");
    }

    if let Some(desc) = tool.input.get("description").and_then(|v| v.as_str()) {
        agent.description = desc.to_string();
        changes.push("description");
    }

    if let Some(content) = tool.input.get("content").and_then(|v| v.as_str()) {
        agent.content = content.to_string();
        changes.push("content");
    }

    if changes.is_empty() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "No changes specified".to_string(),
            is_error: true,
        };
    }

    // Save to disk
    let agent_clone = agent.clone();
    storage::save_prompt_to_dir(&storage::dir_for(PromptType::Agent), &agent_clone);

    state.touch_panel(ContextType::System);
    state.touch_panel(ContextType::Library);

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Updated agent '{}': {}", id, changes.join(", ")),
        is_error: false,
    }
}

pub fn delete(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required 'id' parameter".to_string(),
                is_error: true,
            };
        }
    };

    // Cannot delete built-in agents
    if let Some(agent) = state.agents.iter().find(|a| a.id == id)
        && agent.is_builtin
    {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Cannot delete built-in agent '{}'", id),
            is_error: true,
        };
    }

    let idx = match state.agents.iter().position(|a| a.id == id) {
        Some(i) => i,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Agent '{}' not found", id),
                is_error: true,
            };
        }
    };

    let agent = state.agents.remove(idx);
    storage::delete_prompt_from_dir(&storage::dir_for(PromptType::Agent), id);

    // If this was the active agent, switch to default
    if state.active_agent_id.as_deref() == Some(id) {
        state.active_agent_id = Some(library::default_agent_id().to_string());
    }

    state.touch_panel(ContextType::System);
    state.touch_panel(ContextType::Library);

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Deleted agent '{}' ({})", agent.name, id),
        is_error: false,
    }
}

pub fn load(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id = tool.input.get("id").and_then(|v| v.as_str());

    // If id is None or empty, switch to default agent
    if id.is_none() || id.map(|s| s.is_empty()).unwrap_or(true) {
        state.active_agent_id = Some(library::default_agent_id().to_string());
        state.touch_panel(ContextType::System);
        state.touch_panel(ContextType::Library);
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Switched to default agent ({})", library::default_agent_id()),
            is_error: false,
        };
    }

    let id = id.unwrap();

    if !state.agents.iter().any(|a| a.id == id) {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Agent '{}' not found", id),
            is_error: true,
        };
    }

    state.active_agent_id = Some(id.to_string());
    state.touch_panel(ContextType::System);
    state.touch_panel(ContextType::Library);

    let name = state.agents.iter().find(|a| a.id == id).map(|a| a.name.as_str()).unwrap_or("unknown");

    ToolResult { tool_use_id: tool.id.clone(), content: format!("Loaded agent '{}' ({})", name, id), is_error: false }
}
