use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use cp_base::actions::Action;
use cp_base::constants::theme;
use cp_base::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use cp_base::panels::{ContextItem, Panel};
use cp_base::state::{ContextType, MemoryImportance, State, estimate_tokens};
use cp_base::ui::{Cell, render_table};

pub struct MemoryPanel;

impl MemoryPanel {
    /// Format memories for LLM context.
    /// Closed memories: table with ID, tl_dr, importance, labels.
    /// Open memories: YAML-formatted complete information.
    fn format_memories_for_context(state: &State) -> String {
        if state.memories.is_empty() {
            return "No memories".to_string();
        }

        // Sort by importance (critical first)
        let mut sorted: Vec<_> = state.memories.iter().collect();
        sorted.sort_by_key(|m| match m.importance {
            MemoryImportance::Critical => 0,
            MemoryImportance::High => 1,
            MemoryImportance::Medium => 2,
            MemoryImportance::Low => 3,
        });

        let closed: Vec<_> = sorted.iter().filter(|m| !state.open_memory_ids.contains(&m.id)).collect();
        let open: Vec<_> = sorted.iter().filter(|m| state.open_memory_ids.contains(&m.id)).collect();

        let mut output = String::new();

        // Closed memories as table
        if !closed.is_empty() {
            // Compute column widths
            let headers = ["ID", "Summary", "Importance", "Labels"];
            let rows: Vec<[String; 4]> = closed
                .iter()
                .map(|m| {
                    let labels = if m.labels.is_empty() { String::new() } else { m.labels.join(", ") };
                    [m.id.clone(), m.tl_dr.clone(), m.importance.as_str().to_string(), labels]
                })
                .collect();

            let mut widths = headers.map(|h| h.len());
            for row in &rows {
                for (i, cell) in row.iter().enumerate() {
                    widths[i] = widths[i].max(cell.len());
                }
            }

            // Header
            output.push_str(&format!(
                "{:<w0$} │ {:<w1$} │ {:<w2$} │ {:<w3$}\n",
                headers[0],
                headers[1],
                headers[2],
                headers[3],
                w0 = widths[0],
                w1 = widths[1],
                w2 = widths[2],
                w3 = widths[3],
            ));
            // Separator
            output.push_str(&format!(
                "{}─┼─{}─┼─{}─┼─{}\n",
                "─".repeat(widths[0]),
                "─".repeat(widths[1]),
                "─".repeat(widths[2]),
                "─".repeat(widths[3]),
            ));
            // Rows
            for row in &rows {
                output.push_str(&format!(
                    "{:<w0$} │ {:<w1$} │ {:<w2$} │ {:<w3$}\n",
                    row[0],
                    row[1],
                    row[2],
                    row[3],
                    w0 = widths[0],
                    w1 = widths[1],
                    w2 = widths[2],
                    w3 = widths[3],
                ));
            }
        }

        // Open memories as YAML
        if !open.is_empty() {
            if !closed.is_empty() {
                output.push('\n');
            }
            for (i, memory) in open.iter().enumerate() {
                if i > 0 {
                    output.push('\n');
                }
                output.push_str(&format!("{}:\n", memory.id));
                output.push_str(&format!("  tl_dr: {}\n", memory.tl_dr));
                output.push_str(&format!("  importance: {}\n", memory.importance.as_str()));
                if !memory.labels.is_empty() {
                    output.push_str(&format!("  labels: [{}]\n", memory.labels.join(", ")));
                }
                if !memory.contents.is_empty() {
                    output.push_str("  contents: |\n");
                    for line in memory.contents.lines() {
                        output.push_str(&format!("    {}\n", line));
                    }
                }
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
            if ctx.context_type == ContextType::MEMORY {
                ctx.token_count = token_count;
                break;
            }
        }
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_memories_for_context(state);
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type == ContextType::MEMORY)
            .map(|c| (c.id.as_str(), c.last_refresh_ms))
            .unwrap_or(("P4", 0));
        vec![ContextItem::new(id, "Memories", content, last_refresh_ms)]
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let mut text: Vec<Line> = Vec::new();

        if state.memories.is_empty() {
            text.push(Line::from(vec![
                Span::styled(" ", base_style),
                Span::styled("No memories", Style::default().fg(theme::text_muted()).italic()),
            ]));
            return text;
        }

        // Sort by importance (critical first)
        let mut sorted: Vec<_> = state.memories.iter().collect();
        sorted.sort_by_key(|m| match m.importance {
            MemoryImportance::Critical => 0,
            MemoryImportance::High => 1,
            MemoryImportance::Medium => 2,
            MemoryImportance::Low => 3,
        });

        let closed: Vec<_> = sorted.iter().filter(|m| !state.open_memory_ids.contains(&m.id)).collect();
        let open: Vec<_> = sorted.iter().filter(|m| state.open_memory_ids.contains(&m.id)).collect();

        // Closed memories as table
        if !closed.is_empty() {
            let header = [
                Cell::new("ID", Style::default()),
                Cell::new("Summary", Style::default()),
                Cell::new("Importance", Style::default()),
                Cell::new("Labels", Style::default()),
            ];

            let rows: Vec<Vec<Cell>> = closed
                .iter()
                .map(|memory| {
                    let importance_color = match memory.importance {
                        MemoryImportance::Critical => theme::warning(),
                        MemoryImportance::High => theme::accent(),
                        MemoryImportance::Medium => theme::text_secondary(),
                        MemoryImportance::Low => theme::text_muted(),
                    };

                    let labels = if memory.labels.is_empty() { String::new() } else { memory.labels.join(", ") };

                    vec![
                        Cell::new(&memory.id, Style::default().fg(theme::accent_dim())),
                        Cell::new(&memory.tl_dr, Style::default().fg(theme::text())),
                        Cell::new(memory.importance.as_str(), Style::default().fg(importance_color)),
                        Cell::new(labels, Style::default().fg(theme::text_muted())),
                    ]
                })
                .collect();

            text.extend(render_table(&header, &rows, None, 1));
        }

        // Open memories as YAML
        if !open.is_empty() {
            if !closed.is_empty() {
                text.push(Line::from(""));
            }

            let key_style = Style::default().fg(theme::accent_dim());
            let val_style = Style::default().fg(theme::text());
            let muted_style = Style::default().fg(theme::text_secondary());

            for (i, memory) in open.iter().enumerate() {
                if i > 0 {
                    text.push(Line::from(""));
                }

                let importance_color = match memory.importance {
                    MemoryImportance::Critical => theme::warning(),
                    MemoryImportance::High => theme::accent(),
                    MemoryImportance::Medium => theme::text_secondary(),
                    MemoryImportance::Low => theme::text_muted(),
                };

                // ID header
                text.push(Line::from(vec![
                    Span::styled(" ", base_style),
                    Span::styled(format!("{}:", memory.id), Style::default().fg(theme::accent()).bold()),
                ]));

                // tl_dr
                text.push(Line::from(vec![
                    Span::styled("   ", base_style),
                    Span::styled("tl_dr: ", key_style),
                    Span::styled(memory.tl_dr.clone(), val_style),
                ]));

                // importance
                text.push(Line::from(vec![
                    Span::styled("   ", base_style),
                    Span::styled("importance: ", key_style),
                    Span::styled(memory.importance.as_str(), Style::default().fg(importance_color)),
                ]));

                // labels
                if !memory.labels.is_empty() {
                    text.push(Line::from(vec![
                        Span::styled("   ", base_style),
                        Span::styled("labels: ", key_style),
                        Span::styled(format!("[{}]", memory.labels.join(", ")), muted_style),
                    ]));
                }

                // contents
                if !memory.contents.is_empty() {
                    text.push(Line::from(vec![
                        Span::styled("   ", base_style),
                        Span::styled("contents: |", key_style),
                    ]));
                    for line in memory.contents.lines() {
                        text.push(Line::from(vec![
                            Span::styled("     ", base_style),
                            Span::styled(line.to_string(), muted_style),
                        ]));
                    }
                }
            }
        }

        text
    }
}
