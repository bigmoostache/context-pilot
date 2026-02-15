pub mod builtin;
pub(crate) mod tools;
pub mod types;

use crate::core::panels::Panel;
use crate::state::{ContextType, State};
use crate::tool_defs::{ParamType, ToolCategory, ToolDefinition, ToolParam};
use crate::tools::{ToolResult, ToolUse};

use super::Module;

pub struct PresetModule;

impl Module for PresetModule {
    fn id(&self) -> &'static str {
        "preset"
    }
    fn name(&self) -> &'static str {
        "Preset"
    }
    fn description(&self) -> &'static str {
        "Save and load named worker configuration presets"
    }

    fn is_core(&self) -> bool {
        true
    }
    fn is_global(&self) -> bool {
        true
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                id: "preset_snapshot_myself".to_string(),
                name: "Snapshot Preset".to_string(),
                short_desc: "Save current config".to_string(),
                description: "Saves the current worker configuration as a named preset. \
                    Captures: active system prompt, active modules, disabled tools, \
                    per-worker module data (todos, scratchpad, git settings), and dynamic panels. \
                    Does NOT capture messages or global config."
                    .to_string(),
                params: vec![
                    ToolParam::new("name", ParamType::String)
                        .desc("Preset name (alphanumeric and hyphens only, e.g., 'my-preset')")
                        .required(),
                    ToolParam::new("description", ParamType::String)
                        .desc("Description of what this preset is for")
                        .required(),
                    ToolParam::new("replace", ParamType::String).desc(
                        "Name of existing preset to overwrite. Required if a preset with this name already exists.",
                    ),
                ],
                enabled: true,
                category: ToolCategory::System,
            },
            ToolDefinition {
                id: "preset_load".to_string(),
                name: "Load Preset".to_string(),
                short_desc: "Load saved config".to_string(),
                description: "Loads a named preset, replacing the current worker configuration. \
                    Replaces: active system prompt, active modules, disabled tools, \
                    per-worker module data, and dynamic panels. \
                    Messages and conversation history are preserved."
                    .to_string(),
                params: vec![ToolParam::new("name", ParamType::String).desc("Name of the preset to load").required()],
                enabled: true,
                category: ToolCategory::System,
            },
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "preset_snapshot_myself" => Some(self::tools::execute_snapshot(tool, state)),
            "preset_load" => Some(self::tools::execute_load(tool, state)),
            _ => None,
        }
    }

    fn create_panel(&self, _context_type: ContextType) -> Option<Box<dyn Panel>> {
        None
    }
}
