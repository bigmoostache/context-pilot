pub mod types;
pub mod continuation;
pub mod guard_rail;
pub mod engine;
mod panel;
pub(crate) mod tools;

use serde_json::json;

use crate::core::panels::Panel;
use crate::state::{ContextType, State};
use crate::tool_defs::{ToolDefinition, ToolParam, ParamType, ToolCategory};
use crate::tools::{ToolUse, ToolResult};

use self::panel::SpinePanel;
use super::Module;

pub struct SpineModule;

impl Module for SpineModule {
    fn id(&self) -> &'static str { "spine" }
    fn name(&self) -> &'static str { "Spine" }
    fn description(&self) -> &'static str { "Unified auto-continuation and stream control" }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        json!({
            "notifications": state.notifications,
            "next_notification_id": state.next_notification_id,
            "spine_config": state.spine_config,
        })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(arr) = data.get("notifications") {
            if let Ok(v) = serde_json::from_value(arr.clone()) {
                state.notifications = v;
            }
        }
        if let Some(v) = data.get("next_notification_id").and_then(|v| v.as_u64()) {
            state.next_notification_id = v as usize;
        }
        if let Some(cfg) = data.get("spine_config") {
            if let Ok(v) = serde_json::from_value(cfg.clone()) {
                state.spine_config = v;
            }
        }
    }

    fn fixed_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::Spine]
    }

    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> {
        vec![(ContextType::Spine, "Spine", false)]
    }

    fn create_panel(&self, context_type: ContextType) -> Option<Box<dyn Panel>> {
        match context_type {
            ContextType::Spine => Some(Box::new(SpinePanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                id: "notification_mark_processed".to_string(),
                name: "Mark Notification Processed".to_string(),
                short_desc: "Mark notification as handled".to_string(),
                description: "Marks a spine notification as processed, indicating you've addressed it.".to_string(),
                params: vec![
                    ToolParam::new("id", ParamType::String)
                        .desc("Notification ID (e.g., N1)")
                        .required(),
                ],
                enabled: true,
                category: ToolCategory::Spine,
            },
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "notification_mark_processed" => Some(self::tools::execute_mark_processed(tool, state)),
            _ => None,
        }
    }
}
