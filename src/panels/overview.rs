use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use super::{ContextItem, Panel};
use crate::actions::Action;
use crate::constants::{MAX_CONTEXT_TOKENS, SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::state::{ContextType, State, TodoStatus, MemoryImportance};
use crate::ui::{theme, chars, helpers::format_number};

pub struct OverviewPanel;

impl Panel for OverviewPanel {
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
        "Context Overview".to_string()
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        // LLM should see the same overview the user sees
        let total_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
        let max_tokens = MAX_CONTEXT_TOKENS;
        let usage_pct = (total_tokens as f64 / max_tokens as f64 * 100.0).min(100.0);

        let mut output = format!("Context Usage: {} / {} tokens ({:.1}%)\n\n",
            total_tokens, max_tokens, usage_pct);

        output.push_str("Context Elements:\n");
        for ctx in &state.context {
            let type_name = match ctx.context_type {
                ContextType::Conversation => "conversation",
                ContextType::File => "file",
                ContextType::Tree => "tree",
                ContextType::Glob => "glob",
                ContextType::Grep => "grep",
                ContextType::Tmux => "tmux",
                ContextType::Todo => "todo",
                ContextType::Memory => "memory",
                ContextType::Overview => "overview",
            };

            let details = match ctx.context_type {
                ContextType::File => ctx.file_path.as_deref().unwrap_or("").to_string(),
                ContextType::Glob => ctx.glob_pattern.as_deref().unwrap_or("").to_string(),
                ContextType::Grep => ctx.grep_pattern.as_deref().unwrap_or("").to_string(),
                ContextType::Tmux => ctx.tmux_pane_id.as_deref().unwrap_or("").to_string(),
                _ => String::new(),
            };

            if details.is_empty() {
                output.push_str(&format!("  {} {}: {} tokens\n", ctx.id, type_name, ctx.token_count));
            } else {
                output.push_str(&format!("  {} {} ({}): {} tokens\n", ctx.id, type_name, details, ctx.token_count));
            }
        }

        // Statistics
        let user_msgs = state.messages.iter().filter(|m| m.role == "user").count();
        let assistant_msgs = state.messages.iter().filter(|m| m.role == "assistant").count();
        output.push_str(&format!("\nMessages: {} ({} user, {} assistant)\n",
            state.messages.len(), user_msgs, assistant_msgs));

        if !state.todos.is_empty() {
            let done = state.todos.iter().filter(|t| t.status == TodoStatus::Done).count();
            output.push_str(&format!("Todos: {}/{} done\n", done, state.todos.len()));
        }

        if !state.memories.is_empty() {
            output.push_str(&format!("Memories: {}\n", state.memories.len()));
        }

        vec![ContextItem::new("Context Overview", output)]
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let mut text: Vec<Line> = Vec::new();

        // Token usage header
        let total_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
        let max_tokens = MAX_CONTEXT_TOKENS;
        let usage_pct = (total_tokens as f64 / max_tokens as f64 * 100.0).min(100.0);

        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("TOKEN USAGE".to_string(), Style::default().fg(theme::TEXT_MUTED).bold()),
        ]));
        text.push(Line::from(""));

        let current = format_number(total_tokens);
        let max = format_number(max_tokens);
        let pct = format!("{:.1}%", usage_pct);

        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled(current, Style::default().fg(theme::TEXT).bold()),
            Span::styled(" / ".to_string(), Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(max, Style::default().fg(theme::ACCENT).bold()),
            Span::styled(format!(" ({})", pct), Style::default().fg(theme::TEXT_MUTED)),
        ]));

        // Progress bar
        let bar_width = 60usize;
        let filled = ((usage_pct / 100.0) * bar_width as f64) as usize;
        let empty = bar_width.saturating_sub(filled);

        let bar_color = if usage_pct > 80.0 {
            theme::WARNING
        } else {
            theme::ACCENT
        };

        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled(chars::BLOCK_FULL.repeat(filled), Style::default().fg(bar_color)),
            Span::styled(chars::BLOCK_LIGHT.repeat(empty), Style::default().fg(theme::BG_ELEVATED)),
        ]));

        text.push(Line::from(""));
        text.push(Line::from(vec![
            Span::styled(format!(" {}", chars::HORIZONTAL.repeat(60)), Style::default().fg(theme::BORDER)),
        ]));
        text.push(Line::from(""));

        // Context elements header
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("CONTEXT ELEMENTS".to_string(), Style::default().fg(theme::TEXT_MUTED).bold()),
        ]));
        text.push(Line::from(""));

        let id_width = state.context.iter().map(|c| c.id.len()).max().unwrap_or(2);

        for ctx in &state.context {
            let icon = ctx.context_type.icon();
            let type_name = match ctx.context_type {
                ContextType::Conversation => "conversation",
                ContextType::File => "file",
                ContextType::Tree => "tree",
                ContextType::Glob => "glob",
                ContextType::Grep => "grep",
                ContextType::Tmux => "tmux",
                ContextType::Todo => "todo",
                ContextType::Memory => "memory",
                ContextType::Overview => "overview",
            };

            let details = match ctx.context_type {
                ContextType::File => ctx.file_path.as_deref().unwrap_or("").to_string(),
                ContextType::Glob => ctx.glob_pattern.as_deref().unwrap_or("").to_string(),
                ContextType::Grep => ctx.grep_pattern.as_deref().unwrap_or("").to_string(),
                ContextType::Tmux => {
                    let pane = ctx.tmux_pane_id.as_deref().unwrap_or("?");
                    let desc = ctx.tmux_description.as_deref().unwrap_or("");
                    if desc.is_empty() { pane.to_string() } else { format!("{}: {}", pane, desc) }
                }
                _ => String::new(),
            };

            let tokens = format_number(ctx.token_count);
            let shortcut = format!("{:>width$}", &ctx.id, width = id_width);

            let mut spans = vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(format!("{} ", icon), Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(format!("{} ", shortcut), Style::default().fg(theme::ACCENT_DIM)),
                Span::styled(format!("{:<12}", type_name), Style::default().fg(theme::TEXT_SECONDARY)),
                Span::styled(format!("{:>8}", tokens), Style::default().fg(theme::ACCENT)),
            ];

            if !details.is_empty() {
                let max_detail_len = 40usize;
                let truncated_details = if details.len() > max_detail_len {
                    format!("{}...", &details[..max_detail_len.saturating_sub(3)])
                } else {
                    details
                };
                spans.push(Span::styled(format!("  {}", truncated_details), Style::default().fg(theme::TEXT_MUTED)));
            }

            text.push(Line::from(spans));
        }

        text.push(Line::from(""));
        text.push(Line::from(vec![
            Span::styled(format!(" {}", chars::HORIZONTAL.repeat(60)), Style::default().fg(theme::BORDER)),
        ]));
        text.push(Line::from(""));

        // Statistics section
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("STATISTICS".to_string(), Style::default().fg(theme::TEXT_MUTED).bold()),
        ]));
        text.push(Line::from(""));

        let user_msgs = state.messages.iter().filter(|m| m.role == "user").count();
        let assistant_msgs = state.messages.iter().filter(|m| m.role == "assistant").count();
        let total_msgs = state.messages.len();

        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("Messages: ".to_string(), Style::default().fg(theme::TEXT_SECONDARY)),
            Span::styled(format!("{}", total_msgs), Style::default().fg(theme::TEXT).bold()),
            Span::styled(format!(" ({} user, {} assistant)", user_msgs, assistant_msgs), Style::default().fg(theme::TEXT_MUTED)),
        ]));

        let total_todos = state.todos.len();
        if total_todos > 0 {
            let done_todos = state.todos.iter().filter(|t| t.status == TodoStatus::Done).count();
            let in_progress = state.todos.iter().filter(|t| t.status == TodoStatus::InProgress).count();
            let pending = total_todos - done_todos - in_progress;

            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("Todos: ".to_string(), Style::default().fg(theme::TEXT_SECONDARY)),
                Span::styled(format!("{}/{}", done_todos, total_todos), Style::default().fg(theme::SUCCESS).bold()),
                Span::styled(" done".to_string(), Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(format!(", {} in progress, {} pending", in_progress, pending), Style::default().fg(theme::TEXT_MUTED)),
            ]));
        }

        let total_memories = state.memories.len();
        if total_memories > 0 {
            let critical = state.memories.iter().filter(|m| m.importance == MemoryImportance::Critical).count();
            let high = state.memories.iter().filter(|m| m.importance == MemoryImportance::High).count();
            let medium = state.memories.iter().filter(|m| m.importance == MemoryImportance::Medium).count();
            let low = state.memories.iter().filter(|m| m.importance == MemoryImportance::Low).count();

            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("Memories: ".to_string(), Style::default().fg(theme::TEXT_SECONDARY)),
                Span::styled(format!("{}", total_memories), Style::default().fg(theme::TEXT).bold()),
                Span::styled(format!(" ({} critical, {} high, {} medium, {} low)", critical, high, medium, low), Style::default().fg(theme::TEXT_MUTED)),
            ]));
        }

        text
    }
}
