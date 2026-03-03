mod panel;
mod tools;
pub mod types;

pub use types::{QueueState, QueuedToolCall};

use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::{ContextType, State};
use cp_base::tools::{ParamType, ToolDefinition, ToolParam};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::QueuePanel;

pub struct QueueModule;

impl Module for QueueModule {
    fn id(&self) -> &'static str {
        "queue"
    }
    fn name(&self) -> &'static str {
        "Queue"
    }
    fn description(&self) -> &'static str {
        "Batch tool execution queue for atomic operations"
    }

    fn init_state(&self, state: &mut State) {
        state.set_ext(QueueState::new());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(QueueState::new());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let qs = QueueState::get(state);
        serde_json::json!({
            "active": qs.active,
            "queued_calls": qs.queued_calls,
            "next_index": qs.next_index,
        })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        let qs = QueueState::get_mut(state);
        if let Some(active) = data.get("active").and_then(|v| v.as_bool()) {
            qs.active = active;
        }
        if let Some(arr) = data.get("queued_calls")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            qs.queued_calls = v;
        }
        if let Some(v) = data.get("next_index").and_then(|v| v.as_u64()) {
            qs.next_index = v as usize;
        }
    }

    fn fixed_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::new(ContextType::QUEUE)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> {
        vec![(ContextType::new(ContextType::QUEUE), "Queue", false)]
    }

    fn create_panel(&self, context_type: &ContextType) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            ContextType::QUEUE => Some(Box::new(QueuePanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                id: "Queue_activate".to_string(),
                name: "Queue Activate".to_string(),
                short_desc: "Start queueing tool calls".to_string(),
                description: "Activates the tool queue. All subsequent non-Queue tool calls \
                    will be intercepted and queued instead of executing. Queue tools themselves \
                    always execute immediately. Use Queue_execute to flush the queue."
                    .to_string(),
                params: vec![],
                enabled: true,
                reverie_allowed: true,
                category: "Queue".to_string(),
            },
            ToolDefinition {
                id: "Queue_pause".to_string(),
                name: "Queue Pause".to_string(),
                short_desc: "Stop queueing, execute normally".to_string(),
                description: "Pauses the queue — tool calls execute normally again. \
                    The existing queue stays intact for later flush or empty."
                    .to_string(),
                params: vec![],
                enabled: true,
                reverie_allowed: true,
                category: "Queue".to_string(),
            },
            ToolDefinition {
                id: "Queue_execute".to_string(),
                name: "Queue Execute".to_string(),
                short_desc: "Flush: execute all queued actions".to_string(),
                description: "Executes all queued tool calls in order, atomically. \
                    Returns a summary of results. Clears the queue after execution. \
                    This is the 'commit' — one cache break instead of N."
                    .to_string(),
                params: vec![],
                enabled: true,
                reverie_allowed: true,
                category: "Queue".to_string(),
            },
            ToolDefinition {
                id: "Queue_undo".to_string(),
                name: "Queue Undo".to_string(),
                short_desc: "Remove queued actions by index".to_string(),
                description: "Removes specific queued action(s) by their index number. \
                    Check the Queue panel to see indices."
                    .to_string(),
                params: vec![
                    ToolParam::new("indices", ParamType::Array(Box::new(ParamType::Integer)))
                        .desc("Indices of queued actions to remove (e.g., [1, 3])")
                        .required(),
                ],
                enabled: true,
                reverie_allowed: true,
                category: "Queue".to_string(),
            },
            ToolDefinition {
                id: "Queue_empty".to_string(),
                name: "Queue Empty".to_string(),
                short_desc: "Discard all queued actions".to_string(),
                description: "Discards all queued actions without executing them. \
                    The queue is cleared and deactivated."
                    .to_string(),
                params: vec![],
                enabled: true,
                reverie_allowed: true,
                category: "Queue".to_string(),
            },
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "Queue_activate" => Some(tools::execute_activate(tool, state)),
            "Queue_pause" => Some(tools::execute_pause(tool, state)),
            "Queue_undo" => Some(tools::execute_undo(tool, state)),
            "Queue_empty" => Some(tools::execute_empty(tool, state)),
            // Queue_execute is handled in tool_pipeline.rs (needs module dispatch access)
            _ => None,
        }
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::ContextTypeMeta> {
        vec![cp_base::state::ContextTypeMeta {
            context_type: "queue",
            icon_id: "queue",
            is_fixed: true,
            needs_cache: false,
            fixed_order: Some(9),
            display_name: "queue",
            short_name: "queue",
            needs_async_wait: false,
        }]
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Queue", "Batch tool execution queue — queue actions and flush them atomically")]
    }
}
