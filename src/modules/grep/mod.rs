mod panel;
pub mod tools;

use crate::core::panels::Panel;
use crate::state::{ContextType, State};
use crate::tool_defs::{ParamType, ToolCategory, ToolDefinition, ToolParam};
use crate::tools::{ToolResult, ToolUse};

use self::panel::GrepPanel;
use super::Module;

pub struct GrepModule;

impl Module for GrepModule {
    fn id(&self) -> &'static str {
        "grep"
    }
    fn name(&self) -> &'static str {
        "Grep"
    }
    fn description(&self) -> &'static str {
        "Content search across files"
    }

    fn dynamic_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::Grep]
    }

    fn create_panel(&self, context_type: ContextType) -> Option<Box<dyn Panel>> {
        match context_type {
            ContextType::Grep => Some(Box::new(GrepPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                id: "file_grep".to_string(),
                name: "Grep Search".to_string(),
                short_desc: "Search file contents".to_string(),
                description: "Searches file contents for a regex pattern. Results show matching lines with file:line context. Results are added to context and update dynamically.".to_string(),
                params: vec![
                    ToolParam::new("pattern", ParamType::String)
                        .desc("Regex pattern to search for")
                        .required(),
                    ToolParam::new("path", ParamType::String)
                        .desc("Base path to search from")
                        .default_val("."),
                    ToolParam::new("file_pattern", ParamType::String)
                        .desc("Glob pattern to filter files (e.g., '*.rs', '*.ts')"),
                ],
                enabled: true,
                category: ToolCategory::File,
            },
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "file_grep" => Some(self::tools::execute(tool, state)),
            _ => None,
        }
    }
}
