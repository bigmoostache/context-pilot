pub mod continuation;
pub mod engine;
pub mod guard_rail;
mod panel;
pub(crate) mod tools;
pub mod types;

use serde_json::json;

use crate::core::panels::Panel;
use crate::state::{ContextType, State};
use crate::tool_defs::{ParamType, ToolCategory, ToolDefinition, ToolParam};
use crate::tools::{ToolResult, ToolUse};

use self::panel::SpinePanel;
use super::Module;

pub struct SpineModule;

impl Module for SpineModule {
    fn id(&self) -> &'static str {
        "spine"
    }
    fn name(&self) -> &'static str {
        "Spine"
    }
    fn description(&self) -> &'static str {
        "Unified auto-continuation and stream control"
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        // Prune old processed notifications: keep all unprocessed + latest 10 processed
        let mut to_save: Vec<_> = state.notifications.iter().filter(|n| !n.processed).cloned().collect();
        let mut processed: Vec<_> = state.notifications.iter().filter(|n| n.processed).cloned().collect();
        // Keep only the latest 10 processed (they're in chronological order)
        if processed.len() > 10 {
            processed = processed.split_off(processed.len() - 10);
        }
        to_save.extend(processed);
        // Sort by ID number to maintain order
        to_save.sort_by_key(|n| n.id.trim_start_matches('N').parse::<usize>().unwrap_or(0));

        json!({
            "notifications": to_save,
            "next_notification_id": state.next_notification_id,
            "spine_config": state.spine_config,
        })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(arr) = data.get("notifications")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            state.notifications = v;
        }
        if let Some(v) = data.get("next_notification_id").and_then(|v| v.as_u64()) {
            state.next_notification_id = v as usize;
        }
        if let Some(cfg) = data.get("spine_config")
            && let Ok(v) = serde_json::from_value(cfg.clone())
        {
            state.spine_config = v;
        }
        // Prune stale processed notifications on load too
        prune_notifications(&mut state.notifications);
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
            ToolDefinition {
                id: "spine_configure".to_string(),
                name: "Configure Spine".to_string(),
                short_desc: "Configure auto-continuation and guard rails".to_string(),
                description: "Configures the spine module's auto-continuation behavior and guard rail limits. All parameters are optional — only provided values are changed. Guard rail limits accept null to disable.".to_string(),
                params: vec![
                    ToolParam::new("max_tokens_auto_continue", ParamType::Boolean)
                        .desc("Auto-continue when stream hits max_tokens (default: true)"),
                    ToolParam::new("continue_until_todos_done", ParamType::Boolean)
                        .desc("Keep auto-continuing until all todos are done (default: false)"),
                    ToolParam::new("max_output_tokens", ParamType::Integer)
                        .desc("Guard rail: max total output tokens before blocking. Null to disable."),
                    ToolParam::new("max_cost", ParamType::Number)
                        .desc("Guard rail: max cost in USD before blocking. Null to disable."),
                    ToolParam::new("max_duration_secs", ParamType::Integer)
                        .desc("Guard rail: max autonomous duration in seconds. Null to disable."),
                    ToolParam::new("max_messages", ParamType::Integer)
                        .desc("Guard rail: max conversation messages before blocking. Null to disable."),
                    ToolParam::new("max_auto_retries", ParamType::Integer)
                        .desc("Guard rail: max consecutive auto-continuations without human input. Null to disable."),
                    ToolParam::new("reset_counters", ParamType::Boolean)
                        .desc("Reset runtime counters (auto_continuation_count, autonomous_start_ms)"),
                ],
                enabled: true,
                category: ToolCategory::Spine,
            },
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "notification_mark_processed" => Some(self::tools::execute_mark_processed(tool, state)),
            "spine_configure" => Some(self::tools::execute_configure(tool, state)),
            _ => None,
        }
    }
}

/// Prune processed notifications: keep all unprocessed + latest 10 processed.
fn prune_notifications(notifications: &mut Vec<super::spine::types::Notification>) {
    let processed_count = notifications.iter().filter(|n| n.processed).count();
    if processed_count <= 10 {
        return;
    }
    // Find the cutoff: we want to keep only the latest 10 processed.
    // Notifications are in chronological order, so we drop the oldest processed ones.
    let mut processed_seen = 0;
    let drop_count = processed_count - 10;
    notifications.retain(|n| {
        if n.processed {
            processed_seen += 1;
            processed_seen > drop_count
        } else {
            true
        }
    });
}
