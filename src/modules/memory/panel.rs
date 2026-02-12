use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use crate::core::panels::{ContextItem, Panel};
use crate::actions::Action;
use crate::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::state::{estimate_tokens, ContextType, State, MemoryImportance};
use crate::ui::theme;

pub struct MemoryPanel;

impl MemoryPanel {
    /// Format memories for LLM context.
    /// Open memories show full contents; closed memories show only tl_dr + labels.
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
            let is_open = state.open_memory_ids.contains(&memory.id);
            let labels_str = if memory.labels.is_empty() {
                String::new()
            } else {
                format!(" [{}]", memory.labels.join(", "))
            };

            if is_open && !memory.contents.is_empty() {
                // Open: show tl_dr + labels + full contents
                output.push_str(&format!("[{}] {} ({}){}\n{}\n\n",
                    memory.id, memory.tl_dr, memory.importance.as_str(), labels_str, memory.contents));
            } else {
                // Closed: show only tl_dr + importance + labels
                output.push_str(&format!("[{}] {} ({}){}\n",
                    memory.id, memory.tl_dr, memory.importance.as_str(), labels_str));
            }
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
        let (id, last_refresh_ms) = state.context.iter()
            .find(|c| c.context_type == ContextType::Memory)
            .map(|c| (c.id.as_str(), c.last_refresh_ms))
            .unwrap_or(("P4", 0));
        vec![ContextItem::new(id, "Memories", content, last_refresh_ms)]
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let mut text: Vec<Line> = Vec::new();

        if state.memories.is_empty() {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("No memories".to_string(), Style::default().fg(theme::text_muted()).italic()),
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
                let is_open = state.open_memory_ids.contains(&memory.id);

                let importance_color = match memory.importance {
                    MemoryImportance::Critical => theme::warning(),
                    MemoryImportance::High => theme::accent(),
                    MemoryImportance::Medium => theme::text_secondary(),
                    MemoryImportance::Low => theme::text_muted(),
                };

                let importance_badge = match memory.importance {
                    MemoryImportance::Critical => "!!!",
                    MemoryImportance::High => "!! ",
                    MemoryImportance::Medium => "!  ",
                    MemoryImportance::Low => "   ",
                };

                // Header line: importance badge + id + tl_dr
                let mut header_spans = vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(importance_badge.to_string(), Style::default().fg(importance_color).bold()),
                    Span::styled(memory.id.clone(), Style::default().fg(theme::accent_dim())),
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(memory.tl_dr.clone(), Style::default().fg(theme::text())),
                ];

                // Add labels inline if present
                if !memory.labels.is_empty() {
                    header_spans.push(Span::styled(" ".to_string(), base_style));
                    for (i, label) in memory.labels.iter().enumerate() {
                        if i > 0 {
                            header_spans.push(Span::styled(" ".to_string(), base_style));
                        }
                        header_spans.push(Span::styled(
                            format!("[{}]", label),
                            Style::default().fg(theme::accent_dim()),
                        ));
                    }
                }

                text.push(Line::from(header_spans));

                // If open and has contents, show them indented
                if is_open && !memory.contents.is_empty() {
                    for line in memory.contents.lines() {
                        text.push(Line::from(vec![
                            Span::styled("       ".to_string(), base_style), // indent under badge+id
                            Span::styled(line.to_string(), Style::default().fg(theme::text_secondary())),
                        ]));
                    }
                    text.push(Line::from("")); // Blank line after open memory contents
                }
            }
        }

        text
    }
}
