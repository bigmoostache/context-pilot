mod panel;
mod tools;
pub mod types;

pub use types::{QueueState, QueuedToolCall};

use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::{ContextType, State};
use cp_base::tools::{ParamType, PreFlightResult, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::QueuePanel;
use cp_base::cast::SafeCast;

static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> = std::sync::LazyLock::new(|| {
    serde_yaml::from_str(include_str!("../../../yamls/tools/queue.yaml")).expect("Failed to parse queue tool YAML")
});

#[derive(Debug)]
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
            qs.next_index = v.to_usize();
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
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("Queue_activate", t)
                .short_desc("Start queueing tool calls")
                .category("Queue")
                .reverie_allowed(true)
                .build(),
            ToolDefinition::from_yaml("Queue_pause", t)
                .short_desc("Stop queueing, execute normally")
                .category("Queue")
                .reverie_allowed(true)
                .build(),
            ToolDefinition::from_yaml("Queue_execute", t)
                .short_desc("Flush: execute all queued actions")
                .category("Queue")
                .reverie_allowed(true)
                .build(),
            ToolDefinition::from_yaml("Queue_undo", t)
                .short_desc("Remove queued actions by index")
                .category("Queue")
                .reverie_allowed(true)
                .param_array("indices", ParamType::Integer, true)
                .build(),
            ToolDefinition::from_yaml("Queue_empty", t)
                .short_desc("Discard all queued actions")
                .category("Queue")
                .reverie_allowed(true)
                .build(),
        ]
    }
    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<PreFlightResult> {
        let qs = QueueState::get(state);
        match tool.name.as_str() {
            "Queue_activate" => {
                let mut pf = PreFlightResult::new();
                if qs.active {
                    pf.warnings.push("Queue is already active".to_string());
                }
                Some(pf)
            }
            "Queue_pause" => {
                let mut pf = PreFlightResult::new();
                if !qs.active {
                    pf.warnings.push("Queue is not active".to_string());
                }
                Some(pf)
            }
            "Queue_execute" => {
                let mut pf = PreFlightResult::new();
                if qs.queued_calls.is_empty() {
                    pf.warnings.push("Queue is empty — nothing to execute".to_string());
                }
                Some(pf)
            }
            "Queue_undo" => {
                let mut pf = PreFlightResult::new();
                if let Some(indices) = tool.input.get("indices").and_then(|v| v.as_array()) {
                    for idx_val in indices {
                        if let Some(idx) = idx_val.as_i64()
                            && !qs.queued_calls.iter().any(|c| c.index == idx.to_usize())
                        {
                            pf.errors.push(format!("Queue index {} not found", idx));
                        }
                    }
                }
                Some(pf)
            }
            _ => None,
        }
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
