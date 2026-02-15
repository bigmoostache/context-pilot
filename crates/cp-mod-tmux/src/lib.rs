mod panel;
mod tools;

/// Deprecation timer for tmux panels (milliseconds)
pub(crate) const TMUX_DEPRECATION_MS: u64 = 100; // 100ms — capture-pane is a cheap kernel pipe read

use cp_base::panels::Panel;
use cp_base::state::{ContextType, State};
use cp_base::tool_defs::{ParamType, ToolDefinition, ToolParam};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::TmuxPanel;
use cp_base::modules::Module;

pub struct TmuxModule;

impl Module for TmuxModule {
    fn id(&self) -> &'static str {
        "tmux"
    }
    fn name(&self) -> &'static str {
        "Tmux"
    }
    fn description(&self) -> &'static str {
        "Terminal console management via tmux"
    }

    fn dynamic_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::new(ContextType::TMUX)]
    }

    fn create_panel(&self, context_type: &ContextType) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            ContextType::TMUX => Some(Box::new(TmuxPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                id: "console_create".to_string(),
                name: "Create Console".to_string(),
                short_desc: "Add terminal to context".to_string(),
                description: "Creates a console context element to monitor terminal output.".to_string(),
                params: vec![
                    ToolParam::new("pane_id", ParamType::String)
                        .desc("Console pane ID (e.g., %0, %1). Omit to auto-create a new pane."),
                    ToolParam::new("lines", ParamType::Integer)
                        .desc("Number of lines to capture")
                        .default_val("50"),
                    ToolParam::new("description", ParamType::String)
                        .desc("Description of what this console is for"),
                ],
                enabled: true,
                category: "Console".to_string(),
            },
            ToolDefinition {
                id: "console_edit".to_string(),
                name: "Edit Console".to_string(),
                short_desc: "Update console settings".to_string(),
                description: "Updates configuration for an existing console context.".to_string(),
                params: vec![
                    ToolParam::new("context_id", ParamType::String)
                        .desc("Context ID of the console (e.g., P7)")
                        .required(),
                    ToolParam::new("lines", ParamType::Integer)
                        .desc("Number of lines to capture"),
                    ToolParam::new("description", ParamType::String)
                        .desc("New description"),
                ],
                enabled: true,
                category: "Console".to_string(),
            },
            ToolDefinition {
                id: "console_send_keys".to_string(),
                name: "Console Send Keys".to_string(),
                short_desc: "Send keys to terminal".to_string(),
                description: "Sends keystrokes to a console. Use for running commands or interacting with terminal apps. Enter is sent automatically after the keys — do not include it unless you need to send a blank Enter.".to_string(),
                params: vec![
                    ToolParam::new("context_id", ParamType::String)
                        .desc("Context ID of the console (e.g., P7)")
                        .required(),
                    ToolParam::new("keys", ParamType::String)
                        .desc("Keys to send (e.g., 'ls -la' or 'C-c'). Enter is appended automatically.")
                        .required(),
                ],
                enabled: true,
                category: "Console".to_string(),
            },
            ToolDefinition {
                id: "console_sleep".to_string(),
                name: "Sleep".to_string(),
                short_desc: "Wait and refresh".to_string(),
                description: "Pauses execution. Useful for waiting for terminal output or processes to complete.".to_string(),
                params: vec![],
                enabled: true,
                category: "Console".to_string(),
            },
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "console_create" => Some(self::tools::execute_create_pane(tool, state)),
            "console_edit" => Some(self::tools::execute_edit_config(tool, state)),
            "console_send_keys" => Some(self::tools::execute_send_keys(tool, state)),
            "console_sleep" => Some(self::tools::execute_sleep(tool, state)),
            _ => None,
        }
    }
}
