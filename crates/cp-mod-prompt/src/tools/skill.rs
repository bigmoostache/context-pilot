use crate::storage;
use crate::types::{PromptItem, PromptState, PromptType};
use cp_base::state::{ContextType, State, estimate_tokens};
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

    if PromptState::get(state).skills.iter().any(|s| s.id == id) {
        return ToolResult::new(tool.id.clone(), format!("Skill with ID '{}' already exists", id), true);
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
    PromptState::get_mut(state).skills.push(item);

    state.touch_panel(ContextType::new(ContextType::LIBRARY));

    ToolResult::new(tool.id.clone(), format!("Created skill '{}' with ID '{}'", name, id), false)
}

pub(crate) fn delete(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(id) = tool.input.get("id").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing required 'id' parameter".to_string(), true);
    };

    if let Some(skill) = PromptState::get(state).skills.iter().find(|s| s.id == id)
        && skill.is_builtin
    {
        return ToolResult::new(tool.id.clone(), format!("Cannot delete built-in skill '{}'", id), true);
    }

    let ps = PromptState::get_mut(state);
    let Some(idx) = ps.skills.iter().position(|s| s.id == id) else {
        return ToolResult::new(tool.id.clone(), format!("Skill '{}' not found", id), true);
    };

    // If loaded, unload first
    if ps.loaded_skill_ids.contains(&id.to_string()) {
        state.context.retain(|c| c.get_meta_str("skill_prompt_id") != Some(id));
        PromptState::get_mut(state).loaded_skill_ids.retain(|s| s != id);
    }

    let skill = PromptState::get_mut(state).skills.remove(idx);
    storage::delete_prompt_from_dir(&storage::dir_for(PromptType::Skill), id);

    state.touch_panel(ContextType::new(ContextType::LIBRARY));

    ToolResult::new(tool.id.clone(), format!("Deleted skill '{}' ({})", skill.name, id), false)
}

pub(crate) fn load(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) if !id.is_empty() => id,
        _ => {
            return ToolResult::new(tool.id.clone(), "Missing required 'id' parameter".to_string(), true);
        }
    };

    // Check skill exists
    let ps = PromptState::get(state);
    let skill = match ps.skills.iter().find(|s| s.id == id) {
        Some(s) => s.clone(),
        None => {
            return ToolResult::new(tool.id.clone(), format!("Skill '{}' not found", id), true);
        }
    };

    // Check if already loaded
    if ps.loaded_skill_ids.contains(&id.to_string()) {
        return ToolResult::new(tool.id.clone(), format!("Skill '{}' is already loaded", id), true);
    }

    // Create ContextElement for the skill panel
    let panel_id = state.next_available_context_id();
    let content = format!("[{}] {}\n\n{}", skill.id, skill.name, skill.content);
    let tokens = estimate_tokens(&content);
    let uid = format!("UID_{}_P", state.global_next_uid);
    state.global_next_uid += 1;

    let mut elem = cp_base::state::make_default_context_element(
        &panel_id,
        ContextType::new(ContextType::SKILL),
        &skill.name,
        false,
    );
    elem.uid = Some(uid);
    elem.token_count = tokens;
    elem.set_meta("skill_prompt_id", &id.to_string());
    elem.cached_content = Some(content);
    elem.last_refresh_ms = cp_base::panels::now_ms();

    state.context.push(elem);
    PromptState::get_mut(state).loaded_skill_ids.push(id.to_string());

    state.touch_panel(ContextType::new(ContextType::LIBRARY));

    ToolResult::new(
        tool.id.clone(),
        format!("Loaded skill '{}' as {} ({} tokens)", skill.name, panel_id, tokens),
        false,
    )
}

pub(crate) fn unload(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) if !id.is_empty() => id,
        _ => {
            return ToolResult::new(tool.id.clone(), "Missing required 'id' parameter".to_string(), true);
        }
    };

    if !PromptState::get(state).loaded_skill_ids.contains(&id.to_string()) {
        return ToolResult::new(tool.id.clone(), format!("Skill '{}' is not loaded", id), true);
    }

    // Remove the skill panel from context
    let panel_id = state.context.iter().find(|c| c.get_meta_str("skill_prompt_id") == Some(id)).map(|c| c.id.clone());

    state.context.retain(|c| c.get_meta_str("skill_prompt_id") != Some(id));
    PromptState::get_mut(state).loaded_skill_ids.retain(|s| s != id);

    state.touch_panel(ContextType::new(ContextType::LIBRARY));

    let name =
        PromptState::get(state).skills.iter().find(|s| s.id == id).map_or_else(|| id.to_string(), |s| s.name.clone());

    ToolResult::new(
        tool.id.clone(),
        format!("Unloaded skill '{}'{}", name, panel_id.map(|p| format!(" (removed {})", p)).unwrap_or_default()),
        false,
    )
}
