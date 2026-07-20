use std::fs;

use crate::storage;
use crate::types::{PromptState, PromptType};
use cp_base::config::accessors::library;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

/// Dispatch a tool call to the appropriate handler.
pub(crate) fn dispatch(tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
    let _fg = cp_base::flame!("prompt_dispatch");
    match tool.name.as_str() {
        "Behaviour_create" => Some(behaviour_create(tool, state)),
        "agent_load" => Some(agent_load(tool, state)),
        "skill_load" => Some(skill_load(tool, state)),
        _ => None,
    }
}

/// Create a new behaviour (agent, skill, or command) as a `.md` file.
/// Fails if a file with that ID already exists.
fn behaviour_create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let name = tool.input.get("name").and_then(|v| v.as_str()).unwrap_or("").to_owned();
    let type_str = tool.input.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let description = tool.input.get("description").and_then(|v| v.as_str()).unwrap_or("").to_owned();
    let content = tool.input.get("content").and_then(|v| v.as_str()).unwrap_or("").to_owned();

    if name.is_empty() {
        return ToolResult::new(tool.id.clone(), "Missing required 'name' parameter".to_owned(), true);
    }
    if content.is_empty() {
        return ToolResult::new(tool.id.clone(), "Missing required 'content' parameter".to_owned(), true);
    }

    let prompt_type = match type_str {
        "agent" => PromptType::Agent,
        "skill" => PromptType::Skill,
        "command" => PromptType::Command,
        _ => {
            return ToolResult::new(
                tool.id.clone(),
                format!("Invalid type '{type_str}' — must be 'agent', 'skill', or 'command'"),
                true,
            );
        }
    };

    let id = storage::slugify(&name);
    if id.is_empty() {
        return ToolResult::new(
            tool.id.clone(),
            "Name must contain at least one alphanumeric character".to_owned(),
            true,
        );
    }

    let dir = storage::dir_for(prompt_type);
    let path = dir.join(format!("{id}.md"));

    if path.exists() {
        return ToolResult::new(
            tool.id.clone(),
            format!("A {type_str} with ID '{id}' already exists at {}", path.display()),
            true,
        );
    }

    drop(fs::create_dir_all(&dir));
    let file_content = storage::format_prompt_file(&name, &description, &content);
    if let Err(e) = fs::write(&path, &file_content) {
        return ToolResult::new(tool.id.clone(), format!("Failed to write file: {e}"), true);
    }

    state.touch_panel(Kind::LIBRARY);
    if prompt_type == PromptType::Agent {
        state.touch_panel(Kind::SYSTEM);
    }

    ToolResult::new(tool.id.clone(), format!("Created {type_str} '{name}' (ID: {id}) at {}", path.display()), false)
}

/// Set the active agent by ID, or revert to the default agent.
fn agent_load(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id_opt = tool.input.get("id").and_then(|v| v.as_str());

    let Some(id) = id_opt.filter(|s| !s.is_empty()) else {
        PromptState::get_mut(state).active_agent_id = Some(library::default_agent_id().to_owned());
        state.touch_panel(Kind::SYSTEM);
        state.touch_panel(Kind::LIBRARY);
        return ToolResult::new(
            tool.id.clone(),
            format!("Switched to default agent ({})", library::default_agent_id()),
            false,
        );
    };

    // Dynamically load agents from disk to check existence
    let all_agents = storage::load_prompts_for(PromptType::Agent);
    if !all_agents.iter().any(|a| a.id == id) {
        return ToolResult::new(tool.id.clone(), format!("Agent '{id}' not found"), true);
    }

    PromptState::get_mut(state).active_agent_id = Some(id.to_owned());
    state.touch_panel(Kind::SYSTEM);
    state.touch_panel(Kind::LIBRARY);

    let name = all_agents.iter().find(|a| a.id == id).map_or("unknown", |a| a.name.as_str());
    ToolResult::new(tool.id.clone(), format!("Loaded agent '{name}' ({id})"), false)
}

/// Load a skill into the active context as a panel.
fn skill_load(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) if !id.is_empty() => id,
        _ => {
            return ToolResult::new(tool.id.clone(), "Missing required 'id' parameter".to_owned(), true);
        }
    };

    // Dynamically load skills from disk
    let all_skills = storage::load_prompts_for(PromptType::Skill);
    let skill = match all_skills.iter().find(|s| s.id == id) {
        Some(s) => s.clone(),
        None => {
            return ToolResult::new(tool.id.clone(), format!("Skill '{id}' not found"), true);
        }
    };

    // Check if already loaded
    if PromptState::get(state).loaded_skill_ids.contains(&id.to_owned()) {
        return ToolResult::new(tool.id.clone(), format!("Skill '{id}' is already loaded"), true);
    }

    // Create Entry for the skill panel
    let panel_id = state.next_available_context_id();
    let content = format!("[{}] {}\n\n{}", skill.id, skill.name, skill.content);
    let tokens = estimate_tokens(&content);
    let uid = format!("UID_{}_P", state.global_next_uid);
    state.global_next_uid = state.global_next_uid.saturating_add(1);

    let mut elem = cp_base::state::context::make_default_entry(&panel_id, Kind::new(Kind::SKILL), &skill.name, false);
    elem.uid = Some(uid);
    elem.token_count = tokens;
    elem.set_meta("skill_prompt_id", &id.to_owned());
    elem.cached_content = Some(content);
    elem.last_refresh_ms = cp_base::panels::now_ms();

    state.context.push(elem);
    PromptState::get_mut(state).loaded_skill_ids.push(id.to_owned());

    state.touch_panel(Kind::LIBRARY);

    ToolResult::new(
        tool.id.clone(),
        format!("Loaded skill '{}' as {} ({} tokens)", skill.name, panel_id, tokens),
        false,
    )
}
