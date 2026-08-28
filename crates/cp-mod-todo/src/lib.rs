//! Todo module — hierarchical, thread-owned task tracking with status management.
//!
//! State (`TodoState`) owns all task items; each item carries a compulsory
//! `thread_id` (thread-owned tasks rework). Structural edits and status marking
//! are hosted in the main crate (`Think.todo` + `todo_mark`) which resolves the
//! focused thread and calls the pure ops in [`tools`]; this module exposes the
//! pure task operations + the focus-scoped Todo panel and owns **no tools** of
//! its own. `TodoState` is shared (`is_global == true`).

/// Panel implementation for the todo list view.
mod panel;
/// Focus-scoping + legacy purge (`set_focus_filter`, `purge_threadless`).
pub mod tools;
/// Todo state types: `TodoItem`, `TodoStatus`, `TodoState`.
pub mod types;
/// Virtual-YAML render + diff-apply + reconcile (the `Todo` tool's core).
pub mod yaml;

use types::{TodoState, TodoStatus};

use serde_json::json;

use cp_base::modules::ToolVisualizer;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::ToolDefinition;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::TodoPanel;
use cp_base::cast::Safe as _;
use cp_base::modules::Module;

/// Todo module: hierarchical task tracking with status and nesting.
#[derive(Debug, Clone, Copy)]
pub struct TodoModule;

impl Default for TodoModule {
    fn default() -> Self {
        Self::new()
    }
}

impl TodoModule {
    /// Construct the module marker (funnels cross-crate construction of this
    /// `non_exhaustive` unit struct through an associated fn).
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

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

    fn init_state(&self, state: &mut State) {
        state.set_ext(TodoState::new());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(TodoState::new());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let ts = TodoState::get(state);
        json!({
            "todos": ts.todos,
            "next_todo_id": ts.next_todo_id,
        })
    }
    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        let ts = TodoState::get_mut(state);
        if let Some(arr) = data.get("todos")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            ts.todos = v;
        }
        if let Some(v) = data.get("next_todo_id").and_then(serde_json::Value::as_u64) {
            ts.next_todo_id = v.to_usize();
        }
        // FR4: forever-purge legacy items lacking a thread_id (old schema).
        tools::purge_threadless(state);
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(Kind::TODO)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![(Kind::new(Kind::TODO), "WIP", false)]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            Kind::TODO => Some(Box::new(TodoPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        // No tools of its own: `Think.todo` + `todo_mark` are hosted in the main
        // crate (they need the focused thread from `cp-mod-threads::FocusState`).
        vec![]
    }

    fn pre_flight(&self, _tool: &ToolUse, _state: &State) -> Option<Verdict> {
        None
    }

    fn execute_tool(&self, _tool: &ToolUse, _state: &mut State) -> Option<ToolResult> {
        None
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![cp_base::state::context::TypeMeta {
            context_type: "todo",
            icon_id: "todo",
            is_fixed: true,
            needs_cache: false,
            fixed_order: Some(0),
            display_name: "todo",
            short_name: "wip",
            needs_async_wait: false,
        }]
    }

    fn overview_context_section(&self, state: &State) -> Option<String> {
        let ts = TodoState::get(state);
        // Global rollup across all threads; cancelled items are excluded from
        // both the numerator and denominator (they are soft-deleted).
        let counted: Vec<&types::TodoItem> = ts.todos.iter().filter(|t| t.status != TodoStatus::Cancelled).collect();
        if counted.is_empty() {
            return None;
        }
        let done = counted.iter().filter(|t| t.status == TodoStatus::Done).count();
        Some(format!("Tasks: {}/{} done\n", done, counted.len()))
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Todo", "Track tasks and progress during the session")]
    }

    fn dependencies(&self) -> &[&'static str] {
        &[]
    }

    fn is_core(&self) -> bool {
        false
    }

    fn is_global(&self) -> bool {
        true
    }

    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
        None
    }

    fn context_detail(&self, _ctx: &cp_base::state::context::Entry) -> Option<String> {
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

    fn on_stream_chunk(&self, _text: &str, _state: &mut State) {}

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
