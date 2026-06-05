use ratatui::{
    prelude::{Constraint, Direction, Frame, Layout, Line, Rect, Span, Style},
    widgets::{Block, Clear, Paragraph},
};

use crate::state::State;
use crate::ui::theme;

use super::commands::{PaletteCommand, get_available_commands};
use cp_base::cast::Safe as _;
use cp_render::conversation::{PaletteEntry, PaletteOverlay};

/// State for the command palette
#[derive(Debug, Clone, Default)]
pub(crate) struct CommandPalette {
    /// Whether the palette is open
    pub is_open: bool,
    /// Current search query
    pub query: String,
    /// Cursor position in query
    pub cursor: usize,
    /// Currently selected index in filtered results
    pub selected: usize,
    /// Cached filtered commands
    pub filtered_commands: Vec<PaletteCommand>,
}

impl CommandPalette {
    /// Create a new empty command palette.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Open the palette
    pub(crate) fn open(&mut self, state: &State) {
        self.is_open = true;
        self.query.clear();
        self.cursor = 0;
        self.selected = 0;
        self.update_filtered(state);
    }

    /// Close the palette
    pub(crate) fn close(&mut self) {
        self.is_open = false;
        self.query.clear();
        self.cursor = 0;
        self.selected = 0;
        self.filtered_commands.clear();
    }

    /// Update the filtered commands based on query
    pub(crate) fn update_filtered(&mut self, state: &State) {
        let all_commands = get_available_commands(state);

        if self.query.is_empty() {
            self.filtered_commands = all_commands;
        } else {
            // Filter and sort by match score
            let mut matched: Vec<_> = all_commands
                .into_iter()
                .filter(|cmd| cmd.matches(&self.query))
                .map(|cmd| {
                    let score = cmd.match_score(&self.query);
                    (cmd, score)
                })
                .collect();

            // Sort by score (descending)
            matched.sort_by_key(|b| std::cmp::Reverse(b.1));

            self.filtered_commands = matched.into_iter().map(|(cmd, _)| cmd).collect();
        }

        // Clamp selected index
        if self.filtered_commands.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.filtered_commands.len().saturating_sub(1));
        }
    }

    /// Insert a character at cursor position
    pub(crate) fn insert_char(&mut self, c: char, state: &State) {
        self.query.insert(self.cursor, c);
        self.cursor = self.cursor.saturating_add(c.len_utf8());
        self.selected = 0; // Reset selection on query change
        self.update_filtered(state);
    }

    /// Delete character before cursor
    pub(crate) fn backspace(&mut self, state: &State) {
        if self.cursor > 0 {
            // Find the previous character boundary
            let prev_boundary = self.query.get(..self.cursor).unwrap_or("").char_indices().last().map_or(0, |(i, _)| i);
            let _r = self.query.remove(prev_boundary);
            self.cursor = prev_boundary;
            self.selected = 0;
            self.update_filtered(state);
        }
    }

    /// Delete character at cursor
    pub(crate) fn delete(&mut self, state: &State) {
        if self.cursor < self.query.len() {
            let _r = self.query.remove(self.cursor);
            self.selected = 0;
            self.update_filtered(state);
        }
    }

    /// Move cursor left
    pub(crate) fn cursor_left(&mut self) {
        if self.cursor > 0 {
            let prev_boundary = self.query.get(..self.cursor).unwrap_or("").char_indices().last().map_or(0, |(i, _)| i);
            self.cursor = prev_boundary;
        }
    }

    /// Move cursor right
    pub(crate) fn cursor_right(&mut self) {
        if self.cursor < self.query.len() {
            let next_boundary = self
                .query
                .get(self.cursor..)
                .unwrap_or("")
                .char_indices()
                .nth(1)
                .map_or(self.query.len(), |(i, _)| self.cursor.saturating_add(i));
            self.cursor = next_boundary;
        }
    }

    /// Move selection up
    pub(crate) const fn select_prev(&mut self) {
        if !self.filtered_commands.is_empty() {
            self.selected = if self.selected == 0 {
                self.filtered_commands.len().saturating_sub(1)
            } else {
                self.selected.saturating_sub(1)
            };
        }
    }

    /// Move selection down
    pub(crate) const fn select_next(&mut self) {
        if !self.filtered_commands.is_empty() {
            self.selected = if self.selected.saturating_add(1) >= self.filtered_commands.len() {
                0
            } else {
                self.selected.saturating_add(1)
            };
        }
    }

    /// Get the currently selected command
    pub(crate) fn get_selected(&self) -> Option<&PaletteCommand> {
        self.filtered_commands.get(self.selected)
    }

    /// Render the command palette via the IR pipeline.
    pub(crate) fn render(&self, frame: &mut Frame<'_>, _state: &State) {
        if !self.is_open {
            return;
        }
        let ir = self.to_ir();
        render_command_palette_from_ir(frame, &ir, frame.area());
    }

    /// Build IR snapshot from current palette state.
    pub(crate) fn to_ir(&self) -> PaletteOverlay {
        PaletteOverlay {
            query: self.query.clone(),
            cursor: self.cursor,
            entries: self
                .filtered_commands
                .iter()
                .map(|cmd| PaletteEntry { label: cmd.label.clone(), description: cmd.description.clone() })
                .collect(),
            selected_index: self.selected,
        }
    }
}

// ── IR Adapter ───────────────────────────────────────────────────────

/// Maximum visible items in the palette dropdown.
const MAX_VISIBLE_ITEMS: usize = 8;

/// Render a command palette overlay from IR data.
fn render_command_palette_from_ir(frame: &mut Frame<'_>, palette: &PaletteOverlay, area: Rect) {
    let width = area.width;
    let items_height = palette.entries.len().min(MAX_VISIBLE_ITEMS).to_u16();
    let height = 2u16.saturating_add(items_height);

    let palette_area = Rect::new(0, 0, width, height);

    // Clear area behind the palette
    frame.render_widget(Clear, palette_area);
    frame.render_widget(Block::default().style(Style::default().bg(theme::bg_surface())), palette_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Input
            Constraint::Min(0),    // Results
            Constraint::Length(1), // Bottom border
        ])
        .split(palette_area);
    debug_assert!(chunks.len() >= 3, "palette layout must have at least 3 chunks");

    // Input line
    let Some(&input_chunk) = chunks.first() else { return };
    render_input_line(frame, palette, width, input_chunk);

    // Results
    let Some(&results_chunk) = chunks.get(1) else { return };
    render_result_lines(frame, palette, width, results_chunk);

    // Bottom border
    let Some(&border_chunk) = chunks.get(2) else { return };
    let border_line = "─".repeat(width.to_usize());
    let border = Paragraph::new(Line::from(Span::styled(border_line, Style::default().fg(theme::border()))))
        .style(Style::default().bg(theme::bg_surface()));
    frame.render_widget(border, border_chunk);
}

/// Render the palette input line with cursor and Esc hint.
fn render_input_line(frame: &mut Frame<'_>, palette: &PaletteOverlay, width: u16, area: Rect) {
    let esc_hint = "  Esc to close";
    let available_width = width.to_usize().saturating_sub(4).saturating_sub(esc_hint.len());

    let input_display = if palette.query.is_empty() {
        let hint_padding = available_width.saturating_add(esc_hint.len()).saturating_sub(17);
        vec![
            Span::styled(" > ", Style::default().fg(theme::accent())),
            Span::styled("Type to search...", Style::default().fg(theme::text_muted())),
            Span::styled(format!("{esc_hint:>hint_padding$}"), Style::default().fg(theme::text_muted())),
        ]
    } else {
        let (before, after) = palette.query.split_at(palette.cursor);
        let query_len = before.len().saturating_add(after.len());
        let padding = available_width.saturating_sub(query_len);
        vec![
            Span::styled(" > ", Style::default().fg(theme::accent())),
            Span::styled(before.to_string(), Style::default().fg(theme::text())),
            Span::styled("│", Style::default().fg(theme::accent())),
            Span::styled(after.to_string(), Style::default().fg(theme::text())),
            Span::styled(
                format!("{:>width$}", esc_hint, width = padding.saturating_add(esc_hint.len())),
                Style::default().fg(theme::text_muted()),
            ),
        ]
    };

    let input_line = Paragraph::new(Line::from(input_display)).style(Style::default().bg(theme::bg_surface()));
    frame.render_widget(input_line, area);
}

/// Render the filtered result lines.
fn render_result_lines(frame: &mut Frame<'_>, palette: &PaletteOverlay, width: u16, area: Rect) {
    let visible_start = if palette.selected_index >= MAX_VISIBLE_ITEMS {
        palette.selected_index.saturating_sub(MAX_VISIBLE_ITEMS).saturating_add(1)
    } else {
        0
    };

    let mut result_lines = Vec::new();
    for (i, entry) in palette.entries.iter().enumerate().skip(visible_start).take(MAX_VISIBLE_ITEMS) {
        let is_selected = i == palette.selected_index;
        let (prefix, style) = if is_selected {
            (" > ", Style::default().fg(theme::accent()).bg(theme::bg_elevated()))
        } else {
            ("   ", Style::default().fg(theme::text_secondary()).bg(theme::bg_surface()))
        };

        let desc_style = if is_selected {
            Style::default().fg(theme::text_muted()).bg(theme::bg_elevated())
        } else {
            Style::default().fg(theme::text_muted()).bg(theme::bg_surface())
        };

        let content_len =
            prefix.len().saturating_add(entry.label.len()).saturating_add(2).saturating_add(entry.description.len());
        let line_padding = (width.to_usize()).saturating_sub(content_len);

        result_lines.push(Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(&entry.label, style),
            Span::styled(format!("  {}", entry.description), desc_style),
            Span::styled(
                " ".repeat(line_padding),
                if is_selected {
                    Style::default().bg(theme::bg_elevated())
                } else {
                    Style::default().bg(theme::bg_surface())
                },
            ),
        ]));
    }

    if result_lines.is_empty() {
        result_lines
            .push(Line::from(Span::styled("   No matching commands", Style::default().fg(theme::text_muted()))));
    }

    let results = Paragraph::new(result_lines).style(Style::default().bg(theme::bg_surface()));
    frame.render_widget(results, area);
}
