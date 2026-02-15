use crate::storage;
use crate::types::{PromptItem, PromptType};
use cp_base::state::{ContextType, State, estimate_tokens};
use cp_base::tools::{ToolResult, ToolUse};

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

    if state.skills.iter().any(|s| s.id == id) {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Skill with ID '{}' already exists", id),
            is_error: true,
        };
    }

    let item = PromptItem {
        id: id.clone(),
        name: name.clone(),
        description,
        content,
        prompt_type: PromptType::Skill,
        is_builtin: false,
    };

    storage::save_prompt_to_dir(&storage::dir_for(PromptType::Skill), &item);
    state.skills.push(item);

    state.touch_panel(ContextType::new(ContextType::LIBRARY));

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Created skill '{}' with ID '{}'", name, id),
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

    let skill = match state.skills.iter_mut().find(|s| s.id == id) {
        Some(s) => s,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Skill '{}' not found", id),
                is_error: true,
            };
        }
    };

    if skill.is_builtin {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Cannot edit built-in skill '{}'", id),
            is_error: true,
        };
    }

    let mut changes = Vec::new();

    if let Some(name) = tool.input.get("name").and_then(|v| v.as_str()) {
        skill.name = name.to_string();
        changes.push("name");
    }

    if let Some(desc) = tool.input.get("description").and_then(|v| v.as_str()) {
        skill.description = desc.to_string();
        changes.push("description");
    }

    if let Some(content) = tool.input.get("content").and_then(|v| v.as_str()) {
        skill.content = content.to_string();
        changes.push("content");
    }

    if changes.is_empty() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "No changes specified".to_string(),
            is_error: true,
        };
    }

    let skill_clone = skill.clone();
    storage::save_prompt_to_dir(&storage::dir_for(PromptType::Skill), &skill_clone);

    // If loaded, update the panel's cached_content
    if state.loaded_skill_ids.contains(&id.to_string()) {
        let content_str = format!("[{}] {}\n\n{}", skill_clone.id, skill_clone.name, skill_clone.content);
        let tokens = estimate_tokens(&content_str);
        if let Some(ctx) = state.context.iter_mut().find(|c| c.skill_prompt_id.as_deref() == Some(id)) {
            ctx.cached_content = Some(content_str);
            ctx.token_count = tokens;
            ctx.name = skill_clone.name.clone();
            ctx.last_refresh_ms = cp_base::panels::now_ms();
        }
    }

    state.touch_panel(ContextType::new(ContextType::LIBRARY));

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Updated skill '{}': {}", id, changes.join(", ")),
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

    if let Some(skill) = state.skills.iter().find(|s| s.id == id)
        && skill.is_builtin
    {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Cannot delete built-in skill '{}'", id),
            is_error: true,
        };
    }

    let idx = match state.skills.iter().position(|s| s.id == id) {
        Some(i) => i,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Skill '{}' not found", id),
                is_error: true,
            };
        }
    };

    // If loaded, unload first
    if state.loaded_skill_ids.contains(&id.to_string()) {
        state.context.retain(|c| c.skill_prompt_id.as_deref() != Some(id));
        state.loaded_skill_ids.retain(|s| s != id);
    }

    let skill = state.skills.remove(idx);
    storage::delete_prompt_from_dir(&storage::dir_for(PromptType::Skill), id);

    state.touch_panel(ContextType::new(ContextType::LIBRARY));

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Deleted skill '{}' ({})", skill.name, id),
        is_error: false,
    }
}

pub fn load(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) if !id.is_empty() => id,
        _ => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required 'id' parameter".to_string(),
                is_error: true,
            };
        }
    };

    // Check skill exists
    let skill = match state.skills.iter().find(|s| s.id == id) {
        Some(s) => s.clone(),
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Skill '{}' not found", id),
                is_error: true,
            };
        }
    };

    // Check if already loaded
    if state.loaded_skill_ids.contains(&id.to_string()) {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Skill '{}' is already loaded", id),
            is_error: true,
        };
    }

    // Create ContextElement for the skill panel
    let panel_id = state.next_available_context_id();
    let content = format!("[{}] {}\n\n{}", skill.id, skill.name, skill.content);
    let tokens = estimate_tokens(&content);
    let uid = format!("UID_{}_P", state.global_next_uid);
    state.global_next_uid += 1;

    let elem = cp_base::state::ContextElement {
        id: panel_id.clone(),
        uid: Some(uid),
        context_type: ContextType::new(ContextType::SKILL),
        name: skill.name.clone(),
        token_count: tokens,
        file_path: None,
        glob_pattern: None,
        glob_path: None,
        grep_pattern: None,
        grep_path: None,
        grep_file_pattern: None,
        tmux_pane_id: None,
        tmux_lines: None,
        tmux_last_keys: None,
        tmux_description: None,
        result_command: None,
        skill_prompt_id: Some(id.to_string()),
        cached_content: Some(content),
        history_messages: None,
        cache_deprecated: false,
        cache_in_flight: false,
        last_refresh_ms: cp_base::panels::now_ms(),
        content_hash: None,
        source_hash: None,
        current_page: 0,
        total_pages: 1,
        full_token_count: 0,
        panel_cache_hit: false,
        panel_total_cost: 0.0,
    };

    state.context.push(elem);
    state.loaded_skill_ids.push(id.to_string());

    state.touch_panel(ContextType::new(ContextType::LIBRARY));

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Loaded skill '{}' as {} ({} tokens)", skill.name, panel_id, tokens),
        is_error: false,
    }
}

pub fn unload(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) if !id.is_empty() => id,
        _ => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required 'id' parameter".to_string(),
                is_error: true,
            };
        }
    };

    if !state.loaded_skill_ids.contains(&id.to_string()) {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Skill '{}' is not loaded", id),
            is_error: true,
        };
    }

    // Remove the skill panel from context
    let panel_id = state.context.iter().find(|c| c.skill_prompt_id.as_deref() == Some(id)).map(|c| c.id.clone());

    state.context.retain(|c| c.skill_prompt_id.as_deref() != Some(id));
    state.loaded_skill_ids.retain(|s| s != id);

    state.touch_panel(ContextType::new(ContextType::LIBRARY));

    let name = state.skills.iter().find(|s| s.id == id).map(|s| s.name.as_str()).unwrap_or(id);

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!(
            "Unloaded skill '{}'{}",
            name,
            panel_id.map(|p| format!(" (removed {})", p)).unwrap_or_default()
        ),
        is_error: false,
    }
}
