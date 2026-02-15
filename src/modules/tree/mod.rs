mod panel;
pub mod tools;
pub mod types;

use serde_json::json;

use crate::core::panels::Panel;
use crate::state::{ContextType, State};
use crate::tool_defs::{ParamType, ToolCategory, ToolDefinition, ToolParam};
use crate::tools::{ToolResult, ToolUse};

use self::panel::TreePanel;
use super::Module;

pub struct TreeModule;

impl Module for TreeModule {
    fn id(&self) -> &'static str {
        "tree"
    }
    fn name(&self) -> &'static str {
        "Tree"
    }
    fn description(&self) -> &'static str {
        "Directory tree view with filtering and descriptions"
    }
    fn is_global(&self) -> bool {
        true
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        json!({
            "tree_filter": state.tree_filter,
            "tree_descriptions": state.tree_descriptions,
        })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(v) = data.get("tree_filter").and_then(|v| v.as_str()) {
            state.tree_filter = v.to_string();
        }
        if let Some(arr) = data.get("tree_descriptions")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            state.tree_descriptions = v;
        }
        // Legacy: load tree_open_folders from global config if present (migration)
        if let Some(arr) = data.get("tree_open_folders")
            && let Ok(v) = serde_json::from_value::<Vec<String>>(arr.clone())
        {
            state.tree_open_folders = v;
            if !state.tree_open_folders.contains(&".".to_string()) {
                state.tree_open_folders.insert(0, ".".to_string());
            }
        }
    }

    fn save_worker_data(&self, state: &State) -> serde_json::Value {
        json!({
            "tree_open_folders": state.tree_open_folders,
        })
    }

    fn load_worker_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(arr) = data.get("tree_open_folders")
            && let Ok(v) = serde_json::from_value::<Vec<String>>(arr.clone())
        {
            state.tree_open_folders = v;
            // Ensure root is always open
            if !state.tree_open_folders.contains(&".".to_string()) {
                state.tree_open_folders.insert(0, ".".to_string());
            }
        }
    }

    fn fixed_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::Tree]
    }

    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> {
        vec![(ContextType::Tree, "Tree", true)]
    }

    fn create_panel(&self, context_type: ContextType) -> Option<Box<dyn Panel>> {
        match context_type {
            ContextType::Tree => Some(Box::new(TreePanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                id: "tree_filter".to_string(),
                name: "Tree Filter".to_string(),
                short_desc: "Configure directory filter".to_string(),
                description: "Edits the gitignore-style filter for the directory tree view.".to_string(),
                params: vec![
                    ToolParam::new("filter", ParamType::String)
                        .desc("Gitignore-style patterns, one per line")
                        .required(),
                ],
                enabled: true,
                category: ToolCategory::Tree,
            },
            ToolDefinition {
                id: "tree_toggle".to_string(),
                name: "Tree Toggle".to_string(),
                short_desc: "Open/close folders".to_string(),
                description: "Opens or closes folders in the directory tree view. Closed folders show child count, open folders show contents.".to_string(),
                params: vec![
                    ToolParam::new("paths", ParamType::Array(Box::new(ParamType::String)))
                        .desc("Folder paths to toggle (e.g., ['src', 'src/ui'])")
                        .required(),
                    ToolParam::new("action", ParamType::String)
                        .desc("Action to perform")
                        .enum_vals(&["open", "close", "toggle"])
                        .default_val("toggle"),
                ],
                enabled: true,
                category: ToolCategory::Tree,
            },
            ToolDefinition {
                id: "tree_describe".to_string(),
                name: "Tree Describe".to_string(),
                short_desc: "Add file/folder descriptions".to_string(),
                description: "Adds or updates descriptions for files and folders in the tree. Descriptions appear next to items. A [!] marker indicates the file changed since description was written.".to_string(),
                params: vec![
                    ToolParam::new("descriptions", ParamType::Array(Box::new(ParamType::Object(vec![
                        ToolParam::new("path", ParamType::String)
                            .desc("File or folder path")
                            .required(),
                        ToolParam::new("description", ParamType::String)
                            .desc("Description text"),
                        ToolParam::new("delete", ParamType::Boolean)
                            .desc("Set true to remove description"),
                    ]))))
                        .desc("Array of path descriptions")
                        .required(),
                ],
                enabled: true,
                category: ToolCategory::Tree,
            },
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "tree_filter" => Some(self::tools::execute_edit_filter(tool, state)),
            "tree_toggle" => Some(self::tools::execute_toggle_folders(tool, state)),
            "tree_describe" => Some(self::tools::execute_describe_files(tool, state)),
            _ => None,
        }
    }
}
