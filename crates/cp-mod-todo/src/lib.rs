mod panel;
mod tools;
pub mod types;
pub mod watcher;

pub use types::{TodoItem, TodoState, TodoStatus};
pub use watcher::TodoWatcher;

use serde_json::json;

use cp_base::modules::ToolVisualizer;
use cp_base::panels::Panel;
use cp_base::state::{ContextType, State};
use cp_base::tools::{ParamType, PreFlightResult, ToolDefinition, ToolParam, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::TodoPanel;
use cp_base::modules::Module;

static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> = std::sync::LazyLock::new(|| {
    serde_yaml::from_str(include_str!("../../../yamls/tools/todo.yaml")).expect("Failed to parse todo tool YAML")
});

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
        if let Some(v) = data.get("next_todo_id").and_then(|v| v.as_u64()) {
            ts.next_todo_id = v as usize;
        }
    }

    fn fixed_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::new(ContextType::TODO)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> {
        vec![(ContextType::new(ContextType::TODO), "WIP", false)]
    }

    fn create_panel(&self, context_type: &ContextType) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            ContextType::TODO => Some(Box::new(TodoPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("todo_create", t)
                .short_desc("Add task items")
                .category("Todo")
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

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<PreFlightResult> {
        match tool.name.as_str() {
            "todo_create" => {
                let mut pf = PreFlightResult::new();
                if let Some(todos) = tool.input.get("todos").and_then(|v| v.as_array()) {
                    let ts = TodoState::get(state);
                    for todo in todos {
                        if let Some(parent_id) = todo.get("parent_id").and_then(|v| v.as_str())
                            && !ts.todos.iter().any(|t| t.id == parent_id)
                        {
                            pf.errors.push(format!("Parent todo '{}' not found", parent_id));
                        }
                    }
                }
                Some(pf)
            }
            "todo_update" => {
                let mut pf = PreFlightResult::new();
                if let Some(updates) = tool.input.get("updates").and_then(|v| v.as_array()) {
                    let ts = TodoState::get(state);
                    for update in updates {
                        if let Some(id) = update.get("id").and_then(|v| v.as_str())
                            && !ts.todos.iter().any(|t| t.id == id)
                        {
                            pf.errors.push(format!("Todo '{}' not found", id));
                        }
                    }
                }
                Some(pf)
            }
            "todo_move" => {
                let mut pf = PreFlightResult::new();
                let ts = TodoState::get(state);
                if let Some(id) = tool.input.get("id").and_then(|v| v.as_str())
                    && !ts.todos.iter().any(|t| t.id == id)
                {
                    pf.errors.push(format!("Todo '{}' not found", id));
                }
                if let Some(after_id) = tool.input.get("after_id").and_then(|v| v.as_str())
                    && !ts.todos.iter().any(|t| t.id == after_id)
                {
                    pf.warnings.push(format!("after_id '{}' not found — will move to top", after_id));
                }
                Some(pf)
            }
            _ => None,
        }
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "todo_create" => Some(self::tools::execute_create(tool, state)),
            "todo_update" => Some(self::tools::execute_update(tool, state)),
            "todo_move" => Some(self::tools::execute_move(tool, state)),
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![
            ("todo_create", visualize_todo_output as ToolVisualizer),
            ("todo_update", visualize_todo_output as ToolVisualizer),
            ("todo_move", visualize_todo_output as ToolVisualizer),
        ]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::ContextTypeMeta> {
        vec![cp_base::state::ContextTypeMeta {
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
}

/// Visualizer for todo tool results.
/// Shows todo status with colored indicators and highlights created/updated item names.
fn visualize_todo_output(content: &str, width: usize) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::prelude::*;

    let success_color = Color::Rgb(80, 250, 123); // Green for done
    let warning_color = Color::Rgb(241, 250, 140); // Yellow for in_progress
    let info_color = Color::Rgb(139, 233, 253); // Cyan for pending
    let error_color = Color::Rgb(255, 85, 85); // Red for deleted/errors
    let secondary_color = Color::Rgb(150, 150, 170); // Gray

    let mut lines = Vec::new();

    for line in content.lines() {
        if line.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        let style = if line.starts_with("Error:") {
            Style::default().fg(error_color)
        } else if line.contains("done") || line.contains("Done") || line.starts_with("Created") {
            Style::default().fg(success_color)
        } else if line.contains("in_progress") || line.contains("in-progress") {
            Style::default().fg(warning_color)
        } else if line.contains("pending") || line.contains("Moved") {
            Style::default().fg(info_color)
        } else if line.contains("deleted") || line.contains("Deleted") {
            Style::default().fg(error_color)
        } else if line.contains("Updated") {
            Style::default().fg(success_color)
        } else if line.starts_with("X") && line.chars().nth(1).is_some_and(|c| c.is_ascii_digit()) {
            // Todo IDs like X1, X2
            Style::default().fg(info_color)
        } else if line.contains("→") {
            // Status changes
            Style::default().fg(secondary_color)
        } else {
            Style::default()
        };

        let display = if line.len() > width {
            format!("{}...", &line[..line.floor_char_boundary(width.saturating_sub(3))])
        } else {
            line.to_string()
        };
        lines.push(Line::from(Span::styled(display, style)));
    }

    lines
}
