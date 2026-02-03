use super::{ToolResult, ToolUse};
use crate::constants::prompts;
use crate::state::{State, SystemItem};

/// Create a new system prompt
pub fn create_system(tool: &ToolUse, state: &mut State) -> ToolResult {
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

    let id = format!("S{}", state.next_system_id);
    state.next_system_id += 1;

    let system = SystemItem {
        id: id.clone(),
        name: name.clone(),
        description,
        content,
    };

    state.systems.push(system);

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Created system prompt '{}' with ID {}", name, id),
        is_error: false,
    }
}

/// Edit an existing system prompt
pub fn edit_system(tool: &ToolUse, state: &mut State) -> ToolResult {
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

    let system = match state.systems.iter_mut().find(|s| s.id == id) {
        Some(s) => s,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("System prompt '{}' not found", id),
                is_error: true,
            };
        }
    };

    let mut changes = Vec::new();

    if let Some(name) = tool.input.get("name").and_then(|v| v.as_str()) {
        system.name = name.to_string();
        changes.push("name");
    }

    if let Some(desc) = tool.input.get("description").and_then(|v| v.as_str()) {
        system.description = desc.to_string();
        changes.push("description");
    }

    if let Some(content) = tool.input.get("content").and_then(|v| v.as_str()) {
        system.content = content.to_string();
        changes.push("content");
    }

    if changes.is_empty() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "No changes specified".to_string(),
            is_error: true,
        };
    }

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Updated system prompt '{}': {}", id, changes.join(", ")),
        is_error: false,
    }
}

/// Delete a system prompt
pub fn delete_system(tool: &ToolUse, state: &mut State) -> ToolResult {
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

    // Cannot delete the default seed
    if id == prompts::DEFAULT_SEED_ID {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Cannot delete the default seed ({})", prompts::DEFAULT_SEED_ID),
            is_error: true,
        };
    }

    let idx = match state.systems.iter().position(|s| s.id == id) {
        Some(i) => i,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("System prompt '{}' not found", id),
                is_error: true,
            };
        }
    };

    let system = state.systems.remove(idx);

    // If this was the active system, switch to default
    if state.active_system_id.as_deref() == Some(id) {
        state.active_system_id = Some(prompts::DEFAULT_SEED_ID.to_string());
    }

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Deleted system prompt '{}' ({})", system.name, id),
        is_error: false,
    }
}

/// Load/activate a system prompt
pub fn load_system(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id = tool.input.get("id").and_then(|v| v.as_str());

    // If id is None or empty, switch to default seed
    if id.is_none() || id.map(|s| s.is_empty()).unwrap_or(true) {
        state.active_system_id = Some(prompts::DEFAULT_SEED_ID.to_string());
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Switched to default seed ({})", prompts::DEFAULT_SEED_ID),
            is_error: false,
        };
    }

    let id = id.unwrap();

    // Verify system exists
    if !state.systems.iter().any(|s| s.id == id) {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("System prompt '{}' not found", id),
            is_error: true,
        };
    }

    state.active_system_id = Some(id.to_string());

    let name = state.systems.iter()
        .find(|s| s.id == id)
        .map(|s| s.name.as_str())
        .unwrap_or("unknown");

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Loaded system prompt '{}' ({})", name, id),
        is_error: false,
    }
}
