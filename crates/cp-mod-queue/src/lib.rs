//! Queue module — batch tool calls and execute them atomically.
//!
//! Four tools: `Queue_pause`, `Queue_execute` (flush),
//! `Queue_undo` (remove by index), `Queue_empty` (discard all). The queue
//! intercepts tool calls in the pipeline, stores them, and replays on flush.
//! Reverie sub-agents get their own activation flag sharing the same queue storage.

/// Queue sidebar panel rendering.
mod panel;
/// Tool execution handlers for queue commands.
mod tools;
/// Queue state types: `QueueState`, `QueuedToolCall`.
pub mod types;

use types::QueueState;

use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ParamType, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::QueuePanel;
use cp_base::cast::Safe as _;

/// Lazily parsed tool descriptions from the queue YAML definition.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/queue.yaml")));

/// Queue module: batch tool calls and flush them atomically.
#[derive(Debug, Clone, Copy)]
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
        if let Some(active) = data.get("active").and_then(serde_json::Value::as_bool) {
            qs.active = active;
        }
        if let Some(arr) = data.get("queued_calls")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            qs.queued_calls = v;
        }
        if let Some(v) = data.get("next_index").and_then(serde_json::Value::as_u64) {
            qs.next_index = v.to_usize();
        }
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(Kind::QUEUE)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![(Kind::new(Kind::QUEUE), "Queue", false)]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            Kind::QUEUE => Some(Box::new(QueuePanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
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
    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<Verdict> {
        let qs = QueueState::get(state);
        match tool.name.as_str() {
            "Queue_pause" => {
                let mut pf = Verdict::new();
                if !qs.active {
                    pf.warnings.push("Queue is not active".to_string());
                }
                Some(pf)
            }
            "Queue_execute" => {
                let mut pf = Verdict::new();
                if qs.queued_calls.is_empty() {
                    pf.warnings.push("Queue is empty — nothing to execute".to_string());
                }
                Some(pf)
            }
            "Queue_undo" => {
                let mut pf = Verdict::new();
                if let Some(indices) = tool.input.get("indices").and_then(|v| v.as_array()) {
                    for idx_val in indices {
                        if let Some(idx) = idx_val.as_i64()
                            && !qs.queued_calls.iter().any(|c| c.index == idx.to_usize())
                        {
                            pf.errors.push(format!("Queue index {idx} not found"));
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
            "Queue_pause" => Some(tools::execute_pause(tool, state)),
            "Queue_undo" => Some(tools::execute_undo(tool, state)),
            "Queue_empty" => Some(tools::execute_empty(tool, state)),
            // Queue_execute is handled in tool_pipeline.rs (needs module dispatch access)
            _ => None,
        }
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![cp_base::state::context::TypeMeta {
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

    fn dependencies(&self) -> &[&'static str] {
        &[]
    }
    fn is_core(&self) -> bool {
        false
    }
    fn is_global(&self) -> bool {
        false
    }
    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }
    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}
    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![]
    }
    fn tool_visualizers(&self) -> Vec<(&'static str, cp_base::modules::ToolVisualizer)> {
        vec![]
    }
    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
        None
    }
    fn context_detail(&self, _ctx: &cp_base::state::context::Entry) -> Option<String> {
        None
    }
    fn overview_context_section(&self, _state: &State) -> Option<String> {
        None
    }
    fn overview_render_sections(&self, _state: &State) -> Vec<(u8, Vec<cp_render::Block>)> {
        vec![]
    }
    fn on_close_context(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _state: &mut State,
    ) -> Option<Result<String, String>> {
        None
    }
    fn on_user_message(&self, _state: &mut State) {}
    fn on_stream_stop(&self, _state: &mut State) {}

    fn on_tool_progress(&self, _tool_name: &str, _input_so_far: &str, _state: &mut State) {}

    fn on_tool_complete(&self, _tool_name: &str, _state: &mut State) {}
    fn watch_paths(&self, _state: &State) -> Vec<cp_base::panels::WatchSpec> {
        vec![]
    }
    fn should_invalidate_on_fs_change(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _changed_path: &str,
        _is_dir_event: bool,
    ) -> bool {
        false
    }
    fn watcher_immediate_refresh(&self) -> bool {
        true
    }
}
