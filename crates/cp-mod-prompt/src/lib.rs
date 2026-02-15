mod library_panel;
pub mod seed;
mod skill_panel;
pub(crate) mod storage;
mod tools;
pub(crate) mod types;

use serde_json::json;

use cp_base::panels::Panel;
use cp_base::state::{ContextType, State};
use cp_base::tool_defs::{ParamType, ToolDefinition, ToolParam};
use cp_base::tools::{ToolResult, ToolUse};

use self::library_panel::LibraryPanel;
use self::skill_panel::SkillPanel;
use cp_base::modules::Module;

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

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        json!({
            "active_agent_id": state.active_agent_id,
            "loaded_skill_ids": state.loaded_skill_ids,
            "library_preview": state.library_preview,
        })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(v) = data.get("active_agent_id") {
            state.active_agent_id = v.as_str().map(String::from);
        }
        // Backwards compatibility: try old field name
        if state.active_agent_id.is_none()
            && let Some(v) = data.get("active_system_id")
        {
            state.active_agent_id = v.as_str().map(String::from);
        }
        if let Some(arr) = data.get("loaded_skill_ids")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            state.loaded_skill_ids = v;
        }
        if let Some(v) = data.get("library_preview")
            && let Ok(lp) = serde_json::from_value(v.clone())
        {
            state.library_preview = lp;
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
        vec![
            // === Agent tools ===
            ToolDefinition {
                id: "agent_create".to_string(),
                name: "Create Agent".to_string(),
                short_desc: "Create agent (system prompt)".to_string(),
                description: "Creates a new agent with a name, description, and system prompt content. Agents define the AI's identity and behavior. ID is auto-generated from the name.".to_string(),
                params: vec![
                    ToolParam::new("name", ParamType::String).desc("Agent name").required(),
                    ToolParam::new("description", ParamType::String).desc("Short description"),
                    ToolParam::new("content", ParamType::String).desc("System prompt content").required(),
                ],
                enabled: true,
                category: "Agent".to_string(),
            },
            ToolDefinition {
                id: "agent_edit".to_string(),
                name: "Edit Agent".to_string(),
                short_desc: "Edit agent".to_string(),
                description: "Edits an existing agent. Can update name, description, or content.".to_string(),
                params: vec![
                    ToolParam::new("id", ParamType::String).desc("Agent ID").required(),
                    ToolParam::new("name", ParamType::String).desc("New name"),
                    ToolParam::new("description", ParamType::String).desc("New description"),
                    ToolParam::new("content", ParamType::String).desc("New content"),
                ],
                enabled: true,
                category: "Agent".to_string(),
            },
            ToolDefinition {
                id: "agent_delete".to_string(),
                name: "Delete Agent".to_string(),
                short_desc: "Delete agent".to_string(),
                description: "Deletes an agent. Built-in agents cannot be deleted. If the deleted agent was active, reverts to default.".to_string(),
                params: vec![
                    ToolParam::new("id", ParamType::String).desc("Agent ID to delete").required(),
                ],
                enabled: true,
                category: "Agent".to_string(),
            },
            ToolDefinition {
                id: "agent_load".to_string(),
                name: "Load Agent".to_string(),
                short_desc: "Activate agent".to_string(),
                description: "Activates an agent as the current system prompt. Pass empty id to revert to default.".to_string(),
                params: vec![
                    ToolParam::new("id", ParamType::String).desc("Agent ID to activate. Empty to use default."),
                ],
                enabled: true,
                category: "Agent".to_string(),
            },
            // === Skill tools ===
            ToolDefinition {
                id: "skill_create".to_string(),
                name: "Create Skill".to_string(),
                short_desc: "Create skill".to_string(),
                description: "Creates a new skill. Skills are loaded as context panels that provide additional instructions or knowledge to the AI.".to_string(),
                params: vec![
                    ToolParam::new("name", ParamType::String).desc("Skill name").required(),
                    ToolParam::new("description", ParamType::String).desc("Short description"),
                    ToolParam::new("content", ParamType::String).desc("Skill content (instructions/knowledge)").required(),
                ],
                enabled: true,
                category: "Skill".to_string(),
            },
            ToolDefinition {
                id: "skill_edit".to_string(),
                name: "Edit Skill".to_string(),
                short_desc: "Edit skill".to_string(),
                description: "Edits an existing skill. If loaded, updates the panel content live.".to_string(),
                params: vec![
                    ToolParam::new("id", ParamType::String).desc("Skill ID").required(),
                    ToolParam::new("name", ParamType::String).desc("New name"),
                    ToolParam::new("description", ParamType::String).desc("New description"),
                    ToolParam::new("content", ParamType::String).desc("New content"),
                ],
                enabled: true,
                category: "Skill".to_string(),
            },
            ToolDefinition {
                id: "skill_delete".to_string(),
                name: "Delete Skill".to_string(),
                short_desc: "Delete skill".to_string(),
                description: "Deletes a skill. If loaded, unloads it first. Built-in skills cannot be deleted.".to_string(),
                params: vec![
                    ToolParam::new("id", ParamType::String).desc("Skill ID to delete").required(),
                ],
                enabled: true,
                category: "Skill".to_string(),
            },
            ToolDefinition {
                id: "skill_load".to_string(),
                name: "Load Skill".to_string(),
                short_desc: "Load skill as panel".to_string(),
                description: "Loads a skill as a context panel. The skill's content becomes visible to the AI as a context block.".to_string(),
                params: vec![
                    ToolParam::new("id", ParamType::String).desc("Skill ID to load").required(),
                ],
                enabled: true,
                category: "Skill".to_string(),
            },
            ToolDefinition {
                id: "skill_unload".to_string(),
                name: "Unload Skill".to_string(),
                short_desc: "Unload skill panel".to_string(),
                description: "Unloads a skill, removing its context panel.".to_string(),
                params: vec![
                    ToolParam::new("id", ParamType::String).desc("Skill ID to unload").required(),
                ],
                enabled: true,
                category: "Skill".to_string(),
            },
            // === Command tools ===
            ToolDefinition {
                id: "command_create".to_string(),
                name: "Create Command".to_string(),
                short_desc: "Create command".to_string(),
                description: "Creates a new command. Commands are inline replacements triggered by /command-name in the input field.".to_string(),
                params: vec![
                    ToolParam::new("name", ParamType::String).desc("Command name").required(),
                    ToolParam::new("description", ParamType::String).desc("Short description"),
                    ToolParam::new("content", ParamType::String).desc("Content to replace the /command with").required(),
                ],
                enabled: true,
                category: "Command".to_string(),
            },
            ToolDefinition {
                id: "command_edit".to_string(),
                name: "Edit Command".to_string(),
                short_desc: "Edit command".to_string(),
                description: "Edits an existing command.".to_string(),
                params: vec![
                    ToolParam::new("id", ParamType::String).desc("Command ID").required(),
                    ToolParam::new("name", ParamType::String).desc("New name"),
                    ToolParam::new("description", ParamType::String).desc("New description"),
                    ToolParam::new("content", ParamType::String).desc("New content"),
                ],
                enabled: true,
                category: "Command".to_string(),
            },
            ToolDefinition {
                id: "command_delete".to_string(),
                name: "Delete Command".to_string(),
                short_desc: "Delete command".to_string(),
                description: "Deletes a command. Built-in commands cannot be deleted.".to_string(),
                params: vec![
                    ToolParam::new("id", ParamType::String).desc("Command ID to delete").required(),
                ],
                enabled: true,
                category: "Command".to_string(),
            },
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        tools::dispatch(tool, state)
    }
}
