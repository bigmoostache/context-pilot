//! Prompt library module — agents, skills, and commands.
//!
//! Eleven tools covering CRUD for agents (system prompts), skills (knowledge
//! panels), and commands (input shortcuts), plus a library editor for inline
//! content editing.

mod library_panel;
/// Built-in agent and skill definitions seeded on first run.
pub mod seed;
mod skill_panel;
pub(crate) mod storage;
mod tools;
/// Prompt item types: `PromptItem`, `PromptState`, `PromptType`.
pub mod types;

pub use types::{PromptItem, PromptState, PromptType};

use serde_json::json;

use cp_base::modules::ToolVisualizer;
use cp_base::panels::Panel;
use cp_base::state::context::ContextType;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::PreFlightResult;
use cp_base::tools::{ParamType, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::library_panel::LibraryPanel;
use self::skill_panel::SkillPanel;
use cp_base::modules::Module;

static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/prompt.yaml")));

/// Prompt library module: agents, skills, commands — the ship's charter.
#[derive(Debug, Clone, Copy)]
pub struct PromptModule;

impl Module for PromptModule {
    fn id(&self) -> &'static str {
        "system"
    }
    fn name(&self) -> &'static str {
        "System"
    }
    fn description(&self) -> &'static str {
        "Prompt library — agents, skills, commands"
    }
    fn is_core(&self) -> bool {
        true
    }
    fn is_global(&self) -> bool {
        true
    }

    fn init_state(&self, state: &mut State) {
        state.set_ext(PromptState::new());
    }
    fn reset_state(&self, state: &mut State) {
        state.set_ext(PromptState::new());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let ps = PromptState::get(state);
        json!({
            "active_agent_id": ps.active_agent_id,
            "loaded_skill_ids": ps.loaded_skill_ids,
        })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        let ps = PromptState::get_mut(state);
        if let Some(v) = data.get("active_agent_id") {
            ps.active_agent_id = v.as_str().map(String::from);
        }
        // Backwards compatibility: try old field name
        if ps.active_agent_id.is_none()
            && let Some(v) = data.get("active_system_id")
        {
            ps.active_agent_id = v.as_str().map(String::from);
        }
        if let Some(arr) = data.get("loaded_skill_ids")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            ps.loaded_skill_ids = v;
        }
    }

    fn fixed_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::new(ContextType::LIBRARY)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> {
        vec![(ContextType::new(ContextType::LIBRARY), "Library", false)]
    }

    fn dynamic_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::new(ContextType::SKILL)]
    }

    fn create_panel(&self, context_type: &ContextType) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            ContextType::LIBRARY => Some(Box::new(LibraryPanel)),
            ContextType::SKILL => Some(Box::new(SkillPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            // === Agent tools ===
            ToolDefinition::from_yaml("agent_create", t)
                .short_desc("Create agent (system prompt)")
                .category("Agent")
                .param("name", ParamType::String, true)
                .param("description", ParamType::String, false)
                .param("content", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("Edit_prompt", t)
                .short_desc("Edit agent/skill/command content")
                .category("Agent")
                .param("id", ParamType::String, true)
                .param("old_string", ParamType::String, true)
                .param("new_string", ParamType::String, true)
                .param("replace_all", ParamType::Boolean, false)
                .build(),
            ToolDefinition::from_yaml("agent_delete", t)
                .short_desc("Delete agent")
                .category("Agent")
                .param("id", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("agent_load", t)
                .short_desc("Activate agent")
                .category("Agent")
                .param("id", ParamType::String, false)
                .build(),
            // === Skill tools ===
            ToolDefinition::from_yaml("skill_create", t)
                .short_desc("Create skill")
                .category("Skill")
                .param("name", ParamType::String, true)
                .param("description", ParamType::String, false)
                .param("content", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("skill_delete", t)
                .short_desc("Delete skill")
                .category("Skill")
                .param("id", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("skill_load", t)
                .short_desc("Load skill as panel")
                .category("Skill")
                .param("id", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("skill_unload", t)
                .short_desc("Unload skill panel")
                .category("Skill")
                .param("id", ParamType::String, true)
                .build(),
            // === Library editor tools ===
            ToolDefinition::from_yaml("Library_open_prompt_editor", t)
                .short_desc("Open prompt in editor")
                .category("Agent")
                .param("id", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("Library_close_prompt_editor", t)
                .short_desc("Close prompt editor")
                .category("Agent")
                .build(),
            // === Command tools ===
            ToolDefinition::from_yaml("command_create", t)
                .short_desc("Create command")
                .category("Command")
                .param("name", ParamType::String, true)
                .param("description", ParamType::String, false)
                .param("content", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("command_delete", t)
                .short_desc("Delete command")
                .category("Command")
                .param("id", ParamType::String, true)
                .build(),
        ]
    }

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<PreFlightResult> {
        let ps = PromptState::get(state);
        match tool.name.as_str() {
            "agent_delete" => {
                let mut pf = PreFlightResult::new();
                if let Some(id) = tool.input.get("id").and_then(|v| v.as_str()) {
                    match ps.agents.iter().find(|a| a.id == id) {
                        None => pf.errors.push(format!("Agent '{id}' not found")),
                        Some(a) if a.is_builtin => {
                            pf.errors.push(format!("Agent '{id}' is built-in and cannot be deleted"));
                        }
                        _ => {}
                    }
                }
                Some(pf)
            }
            "agent_load" => {
                let mut pf = PreFlightResult::new();
                if let Some(id) = tool.input.get("id").and_then(|v| v.as_str())
                    && !id.is_empty()
                    && !ps.agents.iter().any(|a| a.id == id)
                {
                    pf.errors.push(format!("Agent '{id}' not found"));
                }
                Some(pf)
            }
            "skill_delete" => {
                let mut pf = PreFlightResult::new();
                if let Some(id) = tool.input.get("id").and_then(|v| v.as_str()) {
                    match ps.skills.iter().find(|s| s.id == id) {
                        None => pf.errors.push(format!("Skill '{id}' not found")),
                        Some(s) if s.is_builtin => {
                            pf.errors.push(format!("Skill '{id}' is built-in and cannot be deleted"));
                        }
                        _ => {}
                    }
                }
                Some(pf)
            }
            "skill_load" => {
                let mut pf = PreFlightResult::new();
                if let Some(id) = tool.input.get("id").and_then(|v| v.as_str()) {
                    if !ps.skills.iter().any(|s| s.id == id) {
                        pf.errors.push(format!("Skill '{id}' not found"));
                    } else if ps.loaded_skill_ids.contains(&id.to_string()) {
                        pf.warnings.push(format!("Skill '{id}' is already loaded"));
                    }
                }
                Some(pf)
            }
            "skill_unload" => {
                let mut pf = PreFlightResult::new();
                if let Some(id) = tool.input.get("id").and_then(|v| v.as_str()) {
                    if !ps.skills.iter().any(|s| s.id == id) {
                        pf.errors.push(format!("Skill '{id}' not found"));
                    } else if !ps.loaded_skill_ids.contains(&id.to_string()) {
                        pf.warnings.push(format!("Skill '{id}' is not currently loaded"));
                    }
                }
                Some(pf)
            }
            "Edit_prompt" | "Library_open_prompt_editor" => {
                let mut pf = PreFlightResult::new();
                if let Some(id) = tool.input.get("id").and_then(|v| v.as_str()) {
                    let exists = ps.agents.iter().any(|a| a.id == id)
                        || ps.skills.iter().any(|s| s.id == id)
                        || ps.commands.iter().any(|c| c.id == id);
                    if !exists {
                        pf.errors.push(format!("Prompt '{id}' not found (not an agent, skill, or command)"));
                    }
                }
                Some(pf)
            }
            "Library_close_prompt_editor" => {
                let mut pf = PreFlightResult::new();
                if ps.open_prompt_id.is_none() {
                    pf.warnings.push("No prompt editor is currently open".to_string());
                }
                Some(pf)
            }
            "command_delete" => {
                let mut pf = PreFlightResult::new();
                if let Some(id) = tool.input.get("id").and_then(|v| v.as_str()) {
                    match ps.commands.iter().find(|c| c.id == id) {
                        None => pf.errors.push(format!("Command '{id}' not found")),
                        Some(c) if c.is_builtin => {
                            pf.errors.push(format!("Command '{id}' is built-in and cannot be deleted"));
                        }
                        _ => {}
                    }
                }
                Some(pf)
            }
            _ => None,
        }
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        tools::dispatch(tool, state)
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![
            ("agent_create", visualize_prompt_output),
            ("Edit_prompt", cp_mod_files::visualize_diff),
            ("Library_open_prompt_editor", visualize_prompt_output),
            ("Library_close_prompt_editor", visualize_prompt_output),
            ("agent_delete", visualize_prompt_output),
            ("agent_load", visualize_prompt_output),
            ("skill_create", visualize_prompt_output),
            ("skill_delete", visualize_prompt_output),
            ("skill_load", visualize_prompt_output),
            ("skill_unload", visualize_prompt_output),
            ("command_create", visualize_prompt_output),
            ("command_delete", visualize_prompt_output),
        ]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::ContextTypeMeta> {
        vec![
            cp_base::state::context::ContextTypeMeta {
                context_type: "library",
                icon_id: "library",
                is_fixed: true,
                needs_cache: false,
                fixed_order: Some(1),
                display_name: "library",
                short_name: "library",
                needs_async_wait: false,
            },
            cp_base::state::context::ContextTypeMeta {
                context_type: "skill",
                icon_id: "skill",
                is_fixed: false,
                needs_cache: false,
                fixed_order: None,
                display_name: "skill",
                short_name: "skill",
                needs_async_wait: false,
            },
        ]
    }

    fn on_close_context(
        &self,
        ctx: &cp_base::state::context::ContextElement,
        state: &mut State,
    ) -> Option<Result<String, String>> {
        if ctx.context_type.as_str() != ContextType::SKILL {
            return None;
        }
        let name = ctx.name.clone();
        if let Some(skill_id) = ctx.get_meta_str("skill_prompt_id").map(ToString::to_string) {
            PromptState::get_mut(state).loaded_skill_ids.retain(|s| s != &skill_id);
        }
        Some(Ok(format!("skill: {name}")))
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("Skill", "Manage knowledge skills"),
            ("Agent", "Manage system prompt agents"),
            ("Command", "Manage input commands"),
        ]
    }
}

/// Visualizer for prompt/agent/skill/command tool results.
/// Highlights entity names, shows active status, and differentiates CRUD operations visually.
fn visualize_prompt_output(content: &str, width: usize) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::prelude::{Color, Line, Span, Style};

    let success_color = Color::Rgb(80, 250, 123);
    let info_color = Color::Rgb(139, 233, 253);
    let warning_color = Color::Rgb(241, 250, 140);
    let error_color = Color::Rgb(255, 85, 85);

    let mut lines = Vec::new();

    for line in content.lines() {
        if line.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        let style = if line.starts_with("Error:") {
            Style::default().fg(error_color)
        } else if line.starts_with("Created") || line.starts_with("Loaded") {
            Style::default().fg(success_color)
        } else if line.starts_with("Updated") || line.starts_with("Edited") {
            Style::default().fg(info_color)
        } else if line.starts_with("Deleted") || line.starts_with("Unloaded") {
            Style::default().fg(warning_color)
        } else if line.contains("agent") || line.contains("skill") || line.contains("command") {
            Style::default().fg(info_color)
        } else if line.contains('\'') {
            // Entity names in quotes
            Style::default().fg(info_color)
        } else {
            Style::default()
        };

        let display = if line.len() > width {
            format!("{}...", &line.get(..line.floor_char_boundary(width.saturating_sub(3))).unwrap_or(""))
        } else {
            line.to_string()
        };
        lines.push(Line::from(Span::styled(display, style)));
    }

    lines
}
