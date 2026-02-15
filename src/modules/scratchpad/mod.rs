mod panel;
pub(crate) mod tools;
pub mod types;

use serde_json::json;

use crate::core::panels::Panel;
use crate::state::{ContextType, State};
use crate::tool_defs::{ParamType, ToolCategory, ToolDefinition, ToolParam};
use crate::tools::{ToolResult, ToolUse};

use self::panel::ScratchpadPanel;
use super::Module;

pub struct ScratchpadModule;

impl Module for ScratchpadModule {
    fn id(&self) -> &'static str {
        "scratchpad"
    }
    fn name(&self) -> &'static str {
        "Scratchpad"
    }
    fn description(&self) -> &'static str {
        "Temporary note-taking scratchpad cells"
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        json!({
            "scratchpad_cells": state.scratchpad_cells,
            "next_scratchpad_id": state.next_scratchpad_id,
        })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(arr) = data.get("scratchpad_cells")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            state.scratchpad_cells = v;
        }
        if let Some(v) = data.get("next_scratchpad_id").and_then(|v| v.as_u64()) {
            state.next_scratchpad_id = v as usize;
        }
    }

    fn fixed_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::Scratchpad]
    }

    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> {
        vec![(ContextType::Scratchpad, "Scratch", false)]
    }

    fn create_panel(&self, context_type: ContextType) -> Option<Box<dyn Panel>> {
        match context_type {
            ContextType::Scratchpad => Some(Box::new(ScratchpadPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                id: "scratchpad_create_cell".to_string(),
                name: "Create Scratchpad Cell".to_string(),
                short_desc: "Add scratchpad cell".to_string(),
                description: "Creates a new scratchpad cell for storing temporary notes, code snippets, or data during the conversation.".to_string(),
                params: vec![
                    ToolParam::new("cell_title", ParamType::String)
                        .desc("Title for the cell")
                        .required(),
                    ToolParam::new("cell_contents", ParamType::String)
                        .desc("Content to store in the cell")
                        .required(),
                ],
                enabled: true,
                category: ToolCategory::Scratchpad,
            },
            ToolDefinition {
                id: "scratchpad_edit_cell".to_string(),
                name: "Edit Scratchpad Cell".to_string(),
                short_desc: "Modify scratchpad cell".to_string(),
                description: "Edits an existing scratchpad cell. Can update title and/or contents.".to_string(),
                params: vec![
                    ToolParam::new("cell_id", ParamType::String)
                        .desc("Cell ID to edit (e.g., C1)")
                        .required(),
                    ToolParam::new("cell_title", ParamType::String)
                        .desc("New title (omit to keep current)"),
                    ToolParam::new("cell_contents", ParamType::String)
                        .desc("New contents (omit to keep current)"),
                ],
                enabled: true,
                category: ToolCategory::Scratchpad,
            },
            ToolDefinition {
                id: "scratchpad_wipe".to_string(),
                name: "Wipe Scratchpad".to_string(),
                short_desc: "Delete scratchpad cells".to_string(),
                description: "Deletes scratchpad cells by their IDs. Pass an empty array to wipe all cells.".to_string(),
                params: vec![
                    ToolParam::new("cell_ids", ParamType::Array(Box::new(ParamType::String)))
                        .desc("Cell IDs to delete (e.g., ['C1', 'C2']). Empty array deletes all cells.")
                        .required(),
                ],
                enabled: true,
                category: ToolCategory::Scratchpad,
            },
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "scratchpad_create_cell" => Some(self::tools::execute_create(tool, state)),
            "scratchpad_edit_cell" => Some(self::tools::execute_edit(tool, state)),
            "scratchpad_wipe" => Some(self::tools::execute_wipe(tool, state)),
            _ => None,
        }
    }
}
