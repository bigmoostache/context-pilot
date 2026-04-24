//! Todo module — hierarchical task tracking with status management.
//!
//! Three tools: `todo_create` (with optional nesting), `todo_update` (status,
//! name, description, delete), `todo_move` (reorder). Todos are stored per-worker
//! and drive the spine's `continue_until_todos_done` auto-continuation mode.

/// Panel implementation for the todo list view.
mod panel;
/// Tool implementations for creating, updating, and moving todos.
mod tools;
/// Todo state types: `TodoItem`, `TodoStatus`, `TodoState`.
pub mod types;

use types::{TodoState, TodoStatus};

use serde_json::json;

use cp_base::modules::ToolVisualizer;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ParamType, ToolDefinition, ToolParam, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::TodoPanel;
use cp_base::cast::Safe as _;
use cp_base::modules::Module;

/// Lazily parsed tool definitions loaded from the YAML spec.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/todo.yaml")));

/// Todo module: hierarchical task tracking with status and nesting.
#[derive(Debug, Clone, Copy)]
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
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("todo_create", t)
                .short_desc("Add task items")
                .category("Todo")
                .reverie_allowed(true)
                .param_array(
                    "todos",
                    ParamType::Object(vec![
                        ToolParam::new("name", ParamType::String).desc("Todo title").required(),
                        ToolParam::new("description", ParamType::String).desc("Detailed description"),
                        ToolParam::new("parent_id", ParamType::String).desc("Parent todo ID for nesting"),
                    ]),
                    true,
                )
                .build(),
            ToolDefinition::from_yaml("todo_update", t)
                .short_desc("Modify task items")
                .category("Todo")
                .reverie_allowed(true)
                .param_array(
                    "updates",
                    ParamType::Object(vec![
                        ToolParam::new("id", ParamType::String).desc("Todo ID (e.g., X1)").required(),
                        ToolParam::new("status", ParamType::String).desc("New status").enum_vals(&[
                            "pending",
                            "in_progress",
                            "done",
                            "deleted",
                        ]),
                        ToolParam::new("name", ParamType::String).desc("New name"),
                        ToolParam::new("description", ParamType::String).desc("New description"),
                        ToolParam::new("parent_id", ParamType::String).desc("New parent ID, or null to make top-level"),
                        ToolParam::new("delete", ParamType::Boolean).desc("Set true to delete this todo"),
                    ]),
                    true,
                )
                .build(),
            ToolDefinition::from_yaml("todo_move", t)
                .short_desc("Reorder a task")
                .category("Todo")
                .param("id", ParamType::String, true)
                .param("after_id", ParamType::String, false)
                .build(),
        ]
    }

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<Verdict> {
        match tool.name.as_str() {
            "todo_create" => {
                let mut pf = Verdict::new();
                pf.activate_queue = true;
                if let Some(todos) = tool.input.get("todos").and_then(|v| v.as_array()) {
                    let ts = TodoState::get(state);
                    for todo in todos {
                        if let Some(parent_id) = todo.get("parent_id").and_then(|v| v.as_str())
                            && !ts.todos.iter().any(|t| t.id == parent_id)
                        {
                            pf.errors.push(format!("Parent todo '{parent_id}' not found"));
                        }
                    }
                }
                Some(pf)
            }
            "todo_update" => {
                let mut pf = Verdict::new();
                pf.activate_queue = true;
                if let Some(updates) = tool.input.get("updates").and_then(|v| v.as_array()) {
                    let ts = TodoState::get(state);
                    for update in updates {
                        if let Some(id) = update.get("id").and_then(|v| v.as_str())
                            && !ts.todos.iter().any(|t| t.id == id)
                        {
                            pf.errors.push(format!("Todo '{id}' not found"));
                        }
                    }
                }
                Some(pf)
            }
            "todo_move" => {
                let mut pf = Verdict::new();
                pf.activate_queue = true;
                let ts = TodoState::get(state);
                if let Some(id) = tool.input.get("id").and_then(|v| v.as_str())
                    && !ts.todos.iter().any(|t| t.id == id)
                {
                    pf.errors.push(format!("Todo '{id}' not found"));
                }
                if let Some(after_id) = tool.input.get("after_id").and_then(|v| v.as_str())
                    && !ts.todos.iter().any(|t| t.id == after_id)
                {
                    pf.warnings.push(format!("after_id '{after_id}' not found — will move to top"));
                }
                Some(pf)
            }
            _ => None,
        }
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "todo_create" => Some(tools::execute_create(tool, state)),
            "todo_update" => Some(tools::execute_update(tool, state)),
            "todo_move" => Some(tools::execute_move(tool, state)),
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![
            ("todo_create", visualize_todo_output),
            ("todo_update", visualize_todo_output),
            ("todo_move", visualize_todo_output),
        ]
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
        if ts.todos.is_empty() {
            return None;
        }
        let done = ts.todos.iter().filter(|t| t.status == TodoStatus::Done).count();
        Some(format!("Todos: {}/{} done\n", done, ts.todos.len()))
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
        false
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

/// Visualizer for todo tool results.
/// Shows todo status with colored indicators and highlights created/updated item names.
fn visualize_todo_output(content: &str, width: usize) -> Vec<cp_render::Block> {
    use cp_render::{Block, Semantic, Span};

    content
        .lines()
        .map(|line| {
            if line.is_empty() {
                return Block::empty();
            }
            let semantic = if line.starts_with("Error:") {
                Semantic::Error
            } else if line.contains("done") || line.contains("Done") || line.starts_with("Created") {
                Semantic::Success
            } else if line.contains("in_progress") || line.contains("in-progress") {
                Semantic::Warning
            } else if line.contains("pending") || line.contains("Moved") {
                Semantic::Info
            } else if line.contains("deleted") || line.contains("Deleted") {
                Semantic::Error
            } else if line.contains("Updated") {
                Semantic::Success
            } else if line.starts_with('X') && line.chars().nth(1).is_some_and(|c| c.is_ascii_digit()) {
                Semantic::Info
            } else if line.contains("→") {
                Semantic::Muted
            } else {
                Semantic::Default
            };
            let display = if line.len() > width {
                format!("{}...", line.get(..line.floor_char_boundary(width.saturating_sub(3))).unwrap_or(""))
            } else {
                line.to_string()
            };
            Block::Line(vec![Span::styled(display, semantic)])
        })
        .collect()
}
