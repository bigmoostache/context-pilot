use crossterm::event::KeyEvent;
use ratatui::prelude::{Line, Span, Style};
use unicode_width::UnicodeWidthStr;

use cp_base::config::accessors::theme;
use cp_base::panels::{CacheRequest, CacheUpdate, ContextItem, Panel};
use cp_base::state::actions::Action;
use cp_base::state::context::Entry;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;
use cp_base::ui::{Cell, TextCell, render_table, render_table_text};
use ratatui::Frame;
use ratatui::prelude::Rect;

use crate::types::{MemoryImportance, MemoryState};
use cp_base::panels::scroll_key_action;
use std::fmt::Write as _;

/// Panel that renders memory items and provides LLM context.
pub(crate) struct MemoryPanel;

impl MemoryPanel {
    /// Format memories for LLM context.
    /// Closed memories: table with ID, `tl_dr`, importance, labels.
    /// Open memories: YAML-formatted complete information.
    fn format_memories_for_context(state: &State) -> String {
        let ms = MemoryState::get(state);
        if ms.memories.is_empty() {
            return "No memories".to_string();
        }

        // Sort by importance (critical first)
        let mut sorted: Vec<_> = ms.memories.iter().collect();
        sorted.sort_by_key(|m| match m.importance {
            MemoryImportance::Critical => 0,
            MemoryImportance::High => 1,
            MemoryImportance::Medium => 2,
            MemoryImportance::Low => 3,
        });

        let closed: Vec<_> = sorted.iter().filter(|m| !ms.open_memory_ids.contains(&m.id)).collect();
        let open: Vec<_> = sorted.iter().filter(|m| ms.open_memory_ids.contains(&m.id)).collect();

        let mut output = String::new();

        // Closed memories as table using shared renderer
        if !closed.is_empty() {
            let headers = ["ID", "Summary", "Importance", "Labels"];
            let rows: Vec<Vec<TextCell>> = closed
                .iter()
                .map(|m| {
                    let labels = if m.labels.is_empty() { String::new() } else { m.labels.join(", ") };
                    vec![
                        TextCell::left(&m.id),
                        TextCell::left(&m.tl_dr),
                        TextCell::left(m.importance.as_str()),
                        TextCell::left(labels),
                    ]
                })
                .collect();

            output.push_str(&render_table_text(&headers, &rows));
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
                let _r1 = writeln!(output, "{}:", memory.id);
                let _r2 = writeln!(output, "  tl_dr: {}", memory.tl_dr);
                let _r3 = writeln!(output, "  importance: {}", memory.importance.as_str());
                if !memory.labels.is_empty() {
                    let _r4 = writeln!(output, "  labels: [{}]", memory.labels.join(", "));
                }
                if !memory.contents.is_empty() {
                    output.push_str("  contents: |\n");
                    for line in memory.contents.lines() {
                        let _r5 = writeln!(output, "    {line}");
                    }
                }
            }
        }

        output.trim_end().to_string()
    }
}

impl Panel for MemoryPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn title(&self, _state: &State) -> String {
        "Memory".to_string()
    }

    fn refresh(&self, state: &mut State) {
        let memory_content = Self::format_memories_for_context(state);
        let token_count = estimate_tokens(&memory_content);

        for ctx in &mut state.context {
            if ctx.context_type.as_str() == Kind::MEMORY {
                ctx.token_count = token_count;
                let _ = cp_base::panels::update_if_changed(ctx, &memory_content);
                break;
            }
        }
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_memories_for_context(state);
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == Kind::MEMORY)
            .map_or(("P4", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Memories", content, last_refresh_ms)]
    }

    fn needs_cache(&self) -> bool {
        false
    }

    fn refresh_cache(&self, _request: CacheRequest) -> Option<CacheUpdate> {
        None
    }

    fn build_cache_request(&self, _ctx: &Entry, _state: &State) -> Option<CacheRequest> {
        None
    }

    fn apply_cache_update(&self, _update: CacheUpdate, _ctx: &mut Entry, _state: &mut State) -> bool {
        false
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn suicide(&self, _ctx: &Entry, _state: &State) -> bool {
        false
    }

    fn render(&self, _frame: &mut Frame<'_>, _state: &mut State, _area: Rect) {}

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let mut text: Vec<Line<'_>> = Vec::new();
        let ms = MemoryState::get(state);

        if ms.memories.is_empty() {
            text.push(Line::from(vec![
                Span::styled(" ", base_style),
                Span::styled("No memories", Style::default().fg(theme::text_muted()).italic()),
            ]));
            return text;
        }

        // Sort by importance (critical first)
        let mut sorted: Vec<_> = ms.memories.iter().collect();
        sorted.sort_by_key(|m| match m.importance {
            MemoryImportance::Critical => 0,
            MemoryImportance::High => 1,
            MemoryImportance::Medium => 2,
            MemoryImportance::Low => 3,
        });

        let closed: Vec<_> = sorted.iter().filter(|m| !ms.open_memory_ids.contains(&m.id)).collect();
        let open: Vec<_> = sorted.iter().filter(|m| ms.open_memory_ids.contains(&m.id)).collect();

        // Closed memories as table with word-wrapped Summary column
        if !closed.is_empty() {
            // Calculate available width for Summary column
            // Layout: indent(1) + ID col + " │ " + Summary + " │ " + Importance + " │ " + Labels
            let indent = 1usize;
            let separator_width = 3; // " │ "

            // Measure fixed column widths
            let id_width = closed.iter().map(|m| UnicodeWidthStr::width(m.id.as_str())).max().unwrap_or(2).max(2);
            let imp_width =
                closed.iter().map(|m| UnicodeWidthStr::width(m.importance.as_str())).max().unwrap_or(10).max(10); // "Importance" header
            let labels: Vec<String> =
                closed.iter().map(|m| if m.labels.is_empty() { String::new() } else { m.labels.join(", ") }).collect();
            let labels_width = labels.iter().map(|l| UnicodeWidthStr::width(l.as_str())).max().unwrap_or(6).max(6);

            let viewport = state.last_viewport_width as usize;
            let fixed_width = indent
                .saturating_add(id_width)
                .saturating_add(separator_width)
                .saturating_add(separator_width)
                .saturating_add(imp_width)
                .saturating_add(separator_width)
                .saturating_add(labels_width);
            let summary_max = if viewport > fixed_width.saturating_add(20) {
                viewport.saturating_sub(fixed_width)
            } else {
                40 // minimum reasonable width
            };

            // Word-wrap summaries and build multi-row entries
            let mut all_rows: Vec<Vec<Cell>> = Vec::new();
            for (i, memory) in closed.iter().enumerate() {
                let importance_color = match memory.importance {
                    MemoryImportance::Critical => theme::warning(),
                    MemoryImportance::High => theme::accent(),
                    MemoryImportance::Medium => theme::text_secondary(),
                    MemoryImportance::Low => theme::text_muted(),
                };
                let Some(label_str) = labels.get(i) else { continue };
                let wrapped = wrap_text_simple(&memory.tl_dr, summary_max);

                for (line_idx, line) in wrapped.iter().enumerate() {
                    if line_idx == 0 {
                        // First line: show all columns
                        all_rows.push(vec![
                            Cell::new(&memory.id, Style::default().fg(theme::accent_dim())),
                            Cell::new(line, Style::default().fg(theme::text())),
                            Cell::new(memory.importance.as_str(), Style::default().fg(importance_color)),
                            Cell::new(label_str, Style::default().fg(theme::text_muted())),
                        ]);
                    } else {
                        // Continuation lines: empty ID/Importance/Labels, just Summary
                        all_rows.push(vec![
                            Cell::new("", Style::default()),
                            Cell::new(line, Style::default().fg(theme::text())),
                            Cell::new("", Style::default()),
                            Cell::new("", Style::default()),
                        ]);
                    }
                }
            }

            let header = [
                Cell::new("ID", Style::default()),
                Cell::new("Summary", Style::default()),
                Cell::new("Importance", Style::default()),
                Cell::new("Labels", Style::default()),
            ];

            text.extend(render_table(&header, &all_rows, None, 1));
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

/// Simple word-wrap: break text at word boundaries to fit within `max_width`.
/// Uses `UnicodeWidthStr` for correct display width measurement.
fn wrap_text_simple(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    if UnicodeWidthStr::width(text) <= max_width {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0usize;

    for word in text.split_whitespace() {
        let word_width = UnicodeWidthStr::width(word);
        if current_width == 0 {
            current_line.push_str(word);
            current_width = word_width;
        } else if current_width.saturating_add(1).saturating_add(word_width) <= max_width {
            current_line.push(' ');
            current_line.push_str(word);
            current_width = current_width.saturating_add(1).saturating_add(word_width);
        } else {
            lines.push(current_line);
            current_line = word.to_string();
            current_width = word_width;
        }
    }
    if !current_line.is_empty() {
        lines.push(current_line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}
