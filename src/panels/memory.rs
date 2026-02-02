use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use super::{ContextItem, Panel};
use crate::actions::Action;
use crate::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::state::{estimate_tokens, ContextType, State, MemoryImportance};
use crate::ui::theme;

pub struct MemoryPanel;

impl MemoryPanel {
    /// Format memories for LLM context
    fn format_memories_for_context(state: &State) -> String {
        if state.memories.is_empty() {
            return "No memories".to_string();
        }

        // Sort by importance (critical first, then high, medium, low)
        let mut sorted: Vec<_> = state.memories.iter().collect();
        sorted.sort_by(|a, b| {
            let importance_order = |i: &MemoryImportance| match i {
                MemoryImportance::Critical => 0,
                MemoryImportance::High => 1,
                MemoryImportance::Medium => 2,
                MemoryImportance::Low => 3,
            };
            importance_order(&a.importance).cmp(&importance_order(&b.importance))
        });

        let mut output = String::new();
        for memory in sorted {
            output.push_str(&format!("[{}] {} ({})\n", memory.id, memory.content, memory.importance.as_str()));
        }

        output.trim_end().to_string()
    }
}

impl Panel for MemoryPanel {
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
        "Memory".to_string()
    }

    fn refresh(&self, state: &mut State) {
        let memory_content = Self::format_memories_for_context(state);
        let token_count = estimate_tokens(&memory_content);

        for ctx in &mut state.context {
            if ctx.context_type == ContextType::Memory {
                ctx.token_count = token_count;
                break;
            }
        }
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_memories_for_context(state);
        vec![ContextItem::new("Memories", content)]
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let mut text: Vec<Line> = Vec::new();

        if state.memories.is_empty() {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("No memories".to_string(), Style::default().fg(theme::TEXT_MUTED).italic()),
            ]));
        } else {
            let mut sorted_memories: Vec<_> = state.memories.iter().collect();
            sorted_memories.sort_by(|a, b| {
                let importance_order = |i: &MemoryImportance| match i {
                    MemoryImportance::Critical => 0,
                    MemoryImportance::High => 1,
                    MemoryImportance::Medium => 2,
                    MemoryImportance::Low => 3,
                };
                importance_order(&a.importance).cmp(&importance_order(&b.importance))
            });

            for memory in sorted_memories {
                let importance_color = match memory.importance {
                    MemoryImportance::Critical => theme::WARNING,
                    MemoryImportance::High => theme::ACCENT,
                    MemoryImportance::Medium => theme::TEXT_SECONDARY,
                    MemoryImportance::Low => theme::TEXT_MUTED,
                };

                let importance_badge = match memory.importance {
                    MemoryImportance::Critical => "!!!",
                    MemoryImportance::High => "!! ",
                    MemoryImportance::Medium => "!  ",
                    MemoryImportance::Low => "   ",
                };

                text.push(Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(importance_badge.to_string(), Style::default().fg(importance_color).bold()),
                    Span::styled(memory.id.clone(), Style::default().fg(theme::ACCENT_DIM)),
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(memory.content.clone(), Style::default().fg(theme::TEXT)),
                ]));
            }
        }

        text
    }
}
