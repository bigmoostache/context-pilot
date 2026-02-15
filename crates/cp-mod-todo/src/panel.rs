use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use cp_base::actions::Action;
use cp_base::constants::theme;
use cp_base::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use cp_base::panels::{ContextItem, Panel};
use cp_base::state::{ContextType, State, TodoItem, TodoStatus, estimate_tokens};

pub struct TodoPanel;

impl TodoPanel {
    /// Format todos for LLM context
    fn format_todos_for_context(state: &State) -> String {
        if state.todos.is_empty() {
            return "No todos".to_string();
        }

        fn format_todo(todo: &TodoItem, todos: &[TodoItem], indent: usize) -> String {
            let prefix = "  ".repeat(indent);
            let status_char = todo.status.icon();
            let mut line = format!("{}[{}] {} {}", prefix, status_char, todo.id, todo.name);

            if !todo.description.is_empty() {
                line.push_str(&format!(" - {}", todo.description));
            }
            line.push('\n');

            for child in todos.iter().filter(|t| t.parent_id.as_ref() == Some(&todo.id)) {
                line.push_str(&format_todo(child, todos, indent + 1));
            }

            line
        }

        let mut output = String::new();
        for todo in state.todos.iter().filter(|t| t.parent_id.is_none()) {
            output.push_str(&format_todo(todo, &state.todos, 0));
        }

        output.trim_end().to_string()
    }
}

impl Panel for TodoPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        match key.code {
            KeyCode::Up => Some(Action::ScrollUp(SCROLL_ARROW_AMOUNT)),
            KeyCode::Down => Some(Action::ScrollDown(SCROLL_ARROW_AMOUNT)),
            KeyCode::PageUp => Some(Action::ScrollUp(SCROLL_PAGE_AMOUNT)),
            KeyCode::PageDown => Some(Action::ScrollDown(SCROLL_PAGE_AMOUNT)),
            _ => None,
        }
    }

    fn title(&self, _state: &State) -> String {
        "Todo".to_string()
    }

    fn refresh(&self, state: &mut State) {
        let todo_content = Self::format_todos_for_context(state);
        let token_count = estimate_tokens(&todo_content);

        for ctx in &mut state.context {
            if ctx.context_type == ContextType::TODO {
                ctx.token_count = token_count;
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
            .find(|c| c.context_type == ContextType::TODO)
            .map(|c| (c.id.as_str(), c.last_refresh_ms))
            .unwrap_or(("P3", 0));
        vec![ContextItem::new(id, "Todo List", content, last_refresh_ms)]
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let mut text: Vec<Line> = Vec::new();

        if state.todos.is_empty() {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("No todos".to_string(), Style::default().fg(theme::text_muted()).italic()),
            ]));
        } else {
            fn collect_todo_lines(
                todos: &[cp_base::state::TodoItem],
                parent_id: Option<&String>,
                indent: usize,
                lines: &mut Vec<(usize, String, String, TodoStatus, String)>,
            ) {
                for todo in todos.iter().filter(|t| t.parent_id.as_ref() == parent_id) {
                    lines.push((indent, todo.id.clone(), todo.name.clone(), todo.status, todo.description.clone()));
                    collect_todo_lines(todos, Some(&todo.id), indent + 1, lines);
                }
            }

            let mut todo_lines: Vec<(usize, String, String, TodoStatus, String)> = Vec::new();
            collect_todo_lines(&state.todos, None, 0, &mut todo_lines);

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
                    Span::styled(format!("{}", status_char), Style::default().fg(status_color)),
                    Span::styled("] ".to_string(), Style::default().fg(theme::text_muted())),
                    Span::styled(id, Style::default().fg(theme::accent_dim())),
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(name, name_style),
                ]));

                if !description.is_empty() {
                    let desc_prefix = "  ".repeat(indent + 1);
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
