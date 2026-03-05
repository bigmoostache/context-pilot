use crossterm::event::KeyEvent;
use ratatui::prelude::{Line, Span, Style};

use cp_base::config::accessors::theme;
use cp_base::panels::{ContextItem, Panel};
use cp_base::state::actions::Action;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;

use crate::types::{TodoItem, TodoState, TodoStatus};
use cp_base::panels::scroll_key_action;
use std::fmt::Write as _;

/// Flattened todo entry for rendering: (indent, id, name, status, description).
type TodoLine = (usize, String, String, TodoStatus, String);

/// Panel that renders the hierarchical todo list in the sidebar.
pub(crate) struct TodoPanel;

impl TodoPanel {
    /// Format todos for LLM context
    fn format_todos_for_context(state: &State) -> String {
        fn format_todo(todo: &TodoItem, todos: &[TodoItem], indent: usize) -> String {
            let prefix = "  ".repeat(indent);
            let status_char = todo.status.icon();
            let mut line = format!("{}[{}] {} {}", prefix, status_char, todo.id, todo.name);

            if !todo.description.is_empty() {
                let _r = write!(line, " - {}", todo.description);
            }
            line.push('\n');

            for child in todos.iter().filter(|t| t.parent_id.as_ref() == Some(&todo.id)) {
                line.push_str(&format_todo(child, todos, indent.saturating_add(1)));
            }

            line
        }

        let ts = TodoState::get(state);
        if ts.todos.is_empty() {
            return "No todos".to_string();
        }

        let mut output = String::new();
        for todo in ts.todos.iter().filter(|t| t.parent_id.is_none()) {
            output.push_str(&format_todo(todo, &ts.todos, 0));
        }

        output.trim_end().to_string()
    }
}

impl Panel for TodoPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn title(&self, _state: &State) -> String {
        "Todo".to_string()
    }

    fn refresh(&self, state: &mut State) {
        let todo_content = Self::format_todos_for_context(state);
        let token_count = estimate_tokens(&todo_content);

        for ctx in &mut state.context {
            if ctx.context_type.as_str() == Kind::TODO {
                ctx.token_count = token_count;
                let _ = cp_base::panels::update_if_changed(ctx, &todo_content);
                break;
            }
        }
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_todos_for_context(state);
        // Find the Todo context element to get its ID and timestamp
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == Kind::TODO)
            .map_or(("P3", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Todo List", content, last_refresh_ms)]
    }

    fn needs_cache(&self) -> bool {
        false
    }

    fn refresh_cache(&self, _request: cp_base::panels::CacheRequest) -> Option<cp_base::panels::CacheUpdate> {
        None
    }

    fn build_cache_request(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _state: &State,
    ) -> Option<cp_base::panels::CacheRequest> {
        None
    }

    fn apply_cache_update(
        &self,
        _update: cp_base::panels::CacheUpdate,
        _ctx: &mut cp_base::state::context::Entry,
        _state: &mut State,
    ) -> bool {
        false
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn suicide(&self, _ctx: &cp_base::state::context::Entry, _state: &State) -> bool {
        false
    }

    fn render(&self, _frame: &mut ratatui::Frame<'_>, _state: &mut State, _area: ratatui::prelude::Rect) {}

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let mut text: Vec<Line<'_>> = Vec::new();
        let ts = TodoState::get(state);

        if ts.todos.is_empty() {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("No todos".to_string(), Style::default().fg(theme::text_muted()).italic()),
            ]));
        } else {
            fn collect_todo_lines(
                todos: &[TodoItem],
                parent_id: Option<&String>,
                indent: usize,
                lines: &mut Vec<TodoLine>,
            ) {
                for todo in todos.iter().filter(|t| t.parent_id.as_ref() == parent_id) {
                    lines.push((indent, todo.id.clone(), todo.name.clone(), todo.status, todo.description.clone()));
                    collect_todo_lines(todos, Some(&todo.id), indent.saturating_add(1), lines);
                }
            }

            let mut todo_lines: Vec<TodoLine> = Vec::new();
            collect_todo_lines(&ts.todos, None, 0, &mut todo_lines);

            for (indent, id, name, status, description) in todo_lines {
                let prefix = "  ".repeat(indent);
                let (status_char, status_color) = match status {
                    TodoStatus::Pending => (' ', theme::text_muted()),
                    TodoStatus::InProgress => ('~', theme::warning()),
                    TodoStatus::Done => ('x', theme::success()),
                };

                let name_style = if status == TodoStatus::Done {
                    Style::default().fg(theme::text_muted())
                } else {
                    Style::default().fg(theme::text())
                };

                text.push(Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(prefix.clone(), base_style),
                    Span::styled("[".to_string(), Style::default().fg(theme::text_muted())),
                    Span::styled(format!("{status_char}"), Style::default().fg(status_color)),
                    Span::styled("] ".to_string(), Style::default().fg(theme::text_muted())),
                    Span::styled(id, Style::default().fg(theme::accent_dim())),
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(name, name_style),
                ]));

                if !description.is_empty() {
                    let desc_prefix = "  ".repeat(indent.saturating_add(1));
                    text.push(Line::from(vec![
                        Span::styled(" ".to_string(), base_style),
                        Span::styled(desc_prefix, base_style),
                        Span::styled(description, Style::default().fg(theme::text_secondary())),
                    ]));
                }
            }
        }

        text
    }
}
