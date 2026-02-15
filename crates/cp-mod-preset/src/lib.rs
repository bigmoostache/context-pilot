pub mod builtin;
pub mod tools;

use std::collections::HashSet;

use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::{ContextType, State};
use cp_base::tool_defs::{ParamType, ToolDefinition, ToolParam};
use cp_base::tools::{ToolResult, ToolUse};

/// Function pointers for module-registry operations that live in the binary.
/// Injected at construction time so the crate doesn't depend on the binary.
pub struct PresetModule {
    pub(crate) all_modules_fn: fn() -> Vec<Box<dyn Module>>,
    pub(crate) active_tool_defs_fn: fn(&HashSet<String>) -> Vec<ToolDefinition>,
    pub(crate) ensure_defaults_fn: fn(&mut State),
}

impl PresetModule {
    pub fn new(
        all_modules_fn: fn() -> Vec<Box<dyn Module>>,
        active_tool_defs_fn: fn(&HashSet<String>) -> Vec<ToolDefinition>,
        ensure_defaults_fn: fn(&mut State),
    ) -> Self {
        Self { all_modules_fn, active_tool_defs_fn, ensure_defaults_fn }
    }
}

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
                category: "System".to_string(),
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
                category: "System".to_string(),
            },
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "preset_snapshot_myself" => Some(tools::execute_snapshot(tool, state, self.all_modules_fn)),
            "preset_load" => Some(tools::execute_load(
                tool,
                state,
                self.all_modules_fn,
                self.active_tool_defs_fn,
                self.ensure_defaults_fn,
            )),
            _ => None,
        }
    }

    fn create_panel(&self, _context_type: &ContextType) -> Option<Box<dyn Panel>> {
        None
    }
}
