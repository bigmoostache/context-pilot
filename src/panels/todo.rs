use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use super::{ContextItem, Panel};
use crate::actions::Action;
use crate::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::state::{estimate_tokens, ContextType, State, TodoItem, TodoStatus};
use crate::ui::theme;

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
            if ctx.context_type == ContextType::Todo {
                ctx.token_count = token_count;
                break;
            }
        }
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_todos_for_context(state);
        vec![ContextItem::new("Todo List", content)]
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let mut text: Vec<Line> = Vec::new();

        if state.todos.is_empty() {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("No todos".to_string(), Style::default().fg(theme::TEXT_MUTED).italic()),
            ]));
        } else {
            fn collect_todo_lines(
                todos: &[crate::state::TodoItem],
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
                    TodoStatus::Pending => (' ', theme::TEXT_MUTED),
                    TodoStatus::InProgress => ('~', theme::WARNING),
                    TodoStatus::Done => ('x', theme::SUCCESS),
                };

                let name_style = if status == TodoStatus::Done {
                    Style::default().fg(theme::TEXT_MUTED)
                } else {
                    Style::default().fg(theme::TEXT)
                };

                text.push(Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(prefix.clone(), base_style),
                    Span::styled("[".to_string(), Style::default().fg(theme::TEXT_MUTED)),
                    Span::styled(format!("{}", status_char), Style::default().fg(status_color)),
                    Span::styled("] ".to_string(), Style::default().fg(theme::TEXT_MUTED)),
                    Span::styled(id, Style::default().fg(theme::ACCENT_DIM)),
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(name, name_style),
                ]));

                if !description.is_empty() {
                    let desc_prefix = "  ".repeat(indent + 1);
                    text.push(Line::from(vec![
                        Span::styled(" ".to_string(), base_style),
                        Span::styled(desc_prefix, base_style),
                        Span::styled(description, Style::default().fg(theme::TEXT_SECONDARY)),
                    ]));
                }
            }
        }

        text
    }
}
