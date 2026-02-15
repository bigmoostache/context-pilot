mod panel;
pub(crate) mod tools;
pub mod types;

use serde_json::json;

use crate::core::panels::Panel;
use crate::state::{ContextType, State};
use crate::tool_defs::{ParamType, ToolCategory, ToolDefinition, ToolParam};
use crate::tools::{ToolResult, ToolUse};

use self::panel::TodoPanel;
use super::Module;

pub struct TodoModule;

impl Module for TodoModule {
    fn id(&self) -> &'static str {
        "todo"
    }
    fn name(&self) -> &'static str {
        "Todo"
    }
    fn description(&self) -> &'static str {
        "Task management with hierarchical todos"
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        json!({
            "todos": state.todos,
            "next_todo_id": state.next_todo_id,
        })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(arr) = data.get("todos")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            state.todos = v;
        }
        if let Some(v) = data.get("next_todo_id").and_then(|v| v.as_u64()) {
            state.next_todo_id = v as usize;
        }
    }

    fn fixed_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::Todo]
    }

    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> {
        vec![(ContextType::Todo, "WIP", false)]
    }

    fn create_panel(&self, context_type: ContextType) -> Option<Box<dyn Panel>> {
        match context_type {
            ContextType::Todo => Some(Box::new(TodoPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                id: "todo_create".to_string(),
                name: "Create Todos".to_string(),
                short_desc: "Add task items".to_string(),
                description: "Creates one or more todo items. Supports nesting via parent_id.".to_string(),
                params: vec![
                    ToolParam::new("todos", ParamType::Array(Box::new(ParamType::Object(vec![
                        ToolParam::new("name", ParamType::String)
                            .desc("Todo title")
                            .required(),
                        ToolParam::new("description", ParamType::String)
                            .desc("Detailed description"),
                        ToolParam::new("parent_id", ParamType::String)
                            .desc("Parent todo ID for nesting"),
                    ]))))
                        .desc("Array of todos to create")
                        .required(),
                ],
                enabled: true,
                category: ToolCategory::Todo,
            },
            ToolDefinition {
                id: "todo_update".to_string(),
                name: "Update Todos".to_string(),
                short_desc: "Modify task items".to_string(),
                description: "Updates existing todos: change status, name, description, or delete. Use delete:true to remove a todo.".to_string(),
                params: vec![
                    ToolParam::new("updates", ParamType::Array(Box::new(ParamType::Object(vec![
                        ToolParam::new("id", ParamType::String)
                            .desc("Todo ID (e.g., X1)")
                            .required(),
                        ToolParam::new("status", ParamType::String)
                            .desc("New status")
                            .enum_vals(&["pending", "in_progress", "done", "deleted"]),
                        ToolParam::new("name", ParamType::String)
                            .desc("New name"),
                        ToolParam::new("description", ParamType::String)
                            .desc("New description"),
                        ToolParam::new("parent_id", ParamType::String)
                            .desc("New parent ID, or null to make top-level"),
                        ToolParam::new("delete", ParamType::Boolean)
                            .desc("Set true to delete this todo"),
                    ]))))
                        .desc("Array of todo updates")
                        .required(),
                ],
                enabled: true,
                category: ToolCategory::Todo,
            },
            ToolDefinition {
                id: "todo_move".to_string(),
                name: "Move Todo".to_string(),
                short_desc: "Reorder a task".to_string(),
                description: "Moves a todo to a new position in the list. Place it after another todo, or at the top if after_id is null.".to_string(),
                params: vec![
                    ToolParam::new("id", ParamType::String)
                        .desc("Todo ID to move (e.g., X1)")
                        .required(),
                    ToolParam::new("after_id", ParamType::String)
                        .desc("Place after this todo ID. Null or omit to move to top."),
                ],
                enabled: true,
                category: ToolCategory::Todo,
            },
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "todo_create" => Some(self::tools::execute_create(tool, state)),
            "todo_update" => Some(self::tools::execute_update(tool, state)),
            "todo_move" => Some(self::tools::execute_move(tool, state)),
            _ => None,
        }
    }
}
