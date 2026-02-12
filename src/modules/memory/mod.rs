pub mod types;
mod panel;
pub(crate) mod tools;

use serde_json::json;

use crate::core::panels::Panel;
use crate::state::{ContextType, State};
use crate::tool_defs::{ToolDefinition, ToolParam, ParamType, ToolCategory};
use crate::tools::{ToolUse, ToolResult};

use self::panel::MemoryPanel;
use super::Module;

pub struct MemoryModule;

impl Module for MemoryModule {
    fn id(&self) -> &'static str { "memory" }
    fn name(&self) -> &'static str { "Memory" }
    fn description(&self) -> &'static str { "Persistent memory items across conversations" }
    fn is_global(&self) -> bool { true }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        json!({
            "memories": state.memories,
            "next_memory_id": state.next_memory_id,
            "open_memory_ids": state.open_memory_ids,
        })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(arr) = data.get("memories") {
            if let Ok(v) = serde_json::from_value(arr.clone()) {
                state.memories = v;
            }
        }
        if let Some(v) = data.get("next_memory_id").and_then(|v| v.as_u64()) {
            state.next_memory_id = v as usize;
        }
        if let Some(arr) = data.get("open_memory_ids") {
            if let Ok(v) = serde_json::from_value(arr.clone()) {
                state.open_memory_ids = v;
            }
        }
    }

    fn fixed_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::Memory]
    }

    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> {
        vec![(ContextType::Memory, "Memories", false)]
    }

    fn create_panel(&self, context_type: ContextType) -> Option<Box<dyn Panel>> {
        match context_type {
            ContextType::Memory => Some(Box::new(MemoryPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                id: "memory_create".to_string(),
                name: "Create Memories".to_string(),
                short_desc: "Store persistent memories".to_string(),
                description: "Creates memory items for important information to remember across the conversation.".to_string(),
                params: vec![
                    ToolParam::new("memories", ParamType::Array(Box::new(ParamType::Object(vec![
                        ToolParam::new("content", ParamType::String)
                            .desc("Memory content")
                            .required(),
                        ToolParam::new("contents", ParamType::String)
                            .desc("Rich body text (visible when memory is opened)"),
                        ToolParam::new("importance", ParamType::String)
                            .desc("Importance level")
                            .enum_vals(&["low", "medium", "high", "critical"]),
                        ToolParam::new("labels", ParamType::Array(Box::new(ParamType::String)))
                            .desc("Freeform labels for categorization (e.g., ['architecture', 'bug'])"),
                    ]))))
                        .desc("Array of memories to create")
                        .required(),
                ],
                enabled: true,
                category: ToolCategory::Memory,
            },
            ToolDefinition {
                id: "memory_update".to_string(),
                name: "Update Memories".to_string(),
                short_desc: "Modify stored notes".to_string(),
                description: "Updates or deletes existing memory items.".to_string(),
                params: vec![
                    ToolParam::new("updates", ParamType::Array(Box::new(ParamType::Object(vec![
                        ToolParam::new("id", ParamType::String)
                            .desc("Memory ID (e.g., M1)")
                            .required(),
                        ToolParam::new("content", ParamType::String)
                            .desc("New content"),
                        ToolParam::new("contents", ParamType::String)
                            .desc("New rich body text (visible when memory is opened)"),
                        ToolParam::new("importance", ParamType::String)
                            .desc("New importance level")
                            .enum_vals(&["low", "medium", "high", "critical"]),
                        ToolParam::new("labels", ParamType::Array(Box::new(ParamType::String)))
                            .desc("New labels (replaces existing)"),
                        ToolParam::new("open", ParamType::Boolean)
                            .desc("Set true to show full contents in panel, false to show only tl;dr"),
                        ToolParam::new("delete", ParamType::Boolean)
                            .desc("Set true to delete"),
                    ]))))
                        .desc("Array of memory updates")
                        .required(),
                ],
                enabled: true,
                category: ToolCategory::Memory,
            },
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "memory_create" => Some(self::tools::execute_create(tool, state)),
            "memory_update" => Some(self::tools::execute_update(tool, state)),
            _ => None,
        }
    }
}
