use ratatui::{
    prelude::*,
    widgets::{Block, Clear, Paragraph},
};

use crate::state::State;
use crate::ui::theme;

use super::commands::{PaletteCommand, get_available_commands};

/// State for the command palette
#[derive(Debug, Clone, Default)]
pub struct CommandPalette {
    /// Whether the palette is open
    pub is_open: bool,
    /// Current search query
    pub query: String,
    /// Cursor position in query
    pub cursor: usize,
    /// Currently selected index in filtered results
    pub selected: usize,
    /// Cached filtered commands
    filtered_commands: Vec<PaletteCommand>,
}

impl CommandPalette {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the palette
    pub fn open(&mut self, state: &State) {
        self.is_open = true;
        self.query.clear();
        self.cursor = 0;
        self.selected = 0;
        self.update_filtered(state);
    }

    /// Close the palette
    pub fn close(&mut self) {
        self.is_open = false;
        self.query.clear();
        self.cursor = 0;
        self.selected = 0;
        self.filtered_commands.clear();
    }

    /// Update the filtered commands based on query
    pub fn update_filtered(&mut self, state: &State) {
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
            matched.sort_by(|a, b| b.1.cmp(&a.1));

            self.filtered_commands = matched.into_iter().map(|(cmd, _)| cmd).collect();
        }

        // Clamp selected index
        if !self.filtered_commands.is_empty() {
            self.selected = self.selected.min(self.filtered_commands.len() - 1);
        } else {
            self.selected = 0;
        }
    }

    /// Insert a character at cursor position
    pub fn insert_char(&mut self, c: char, state: &State) {
        self.query.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        self.selected = 0; // Reset selection on query change
        self.update_filtered(state);
    }

    /// Delete character before cursor
    pub fn backspace(&mut self, state: &State) {
        if self.cursor > 0 {
            // Find the previous character boundary
            let prev_boundary = self.query[..self.cursor].char_indices().last().map(|(i, _)| i).unwrap_or(0);
            self.query.remove(prev_boundary);
            self.cursor = prev_boundary;
            self.selected = 0;
            self.update_filtered(state);
        }
    }

    /// Delete character at cursor
    pub fn delete(&mut self, state: &State) {
        if self.cursor < self.query.len() {
            self.query.remove(self.cursor);
            self.selected = 0;
            self.update_filtered(state);
        }
    }

    /// Move cursor left
    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            let prev_boundary = self.query[..self.cursor].char_indices().last().map(|(i, _)| i).unwrap_or(0);
            self.cursor = prev_boundary;
        }
    }

    /// Move cursor right
    pub fn cursor_right(&mut self) {
        if self.cursor < self.query.len() {
            let next_boundary = self.query[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.query.len());
            self.cursor = next_boundary;
        }
    }

    /// Move selection up
    pub fn select_prev(&mut self) {
        if !self.filtered_commands.is_empty() {
            self.selected = if self.selected == 0 { self.filtered_commands.len() - 1 } else { self.selected - 1 };
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if !self.filtered_commands.is_empty() {
            self.selected = (self.selected + 1) % self.filtered_commands.len();
        }
    }

    /// Get the currently selected command
    pub fn get_selected(&self) -> Option<&PaletteCommand> {
        self.filtered_commands.get(self.selected)
    }

    /// Render the command palette
    pub fn render(&self, frame: &mut Frame, _state: &State) {
        if !self.is_open {
            return;
        }

        let area = frame.area();

        // Palette dimensions - full width, at top
        let width = area.width;
        let max_visible_items = 8usize;
        let items_height = self.filtered_commands.len().min(max_visible_items) as u16;
        let height = 2 + items_height; // Input line + items + border

        let palette_area = Rect::new(0, 0, width, height);

        // Clear the area behind the palette
        frame.render_widget(Clear, palette_area);

        // Background fill
        let bg_block = Block::default().style(Style::default().bg(theme::bg_surface()));
        frame.render_widget(bg_block, palette_area);

        // Split area: input line + results + bottom border
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Input
                Constraint::Min(0),    // Results
                Constraint::Length(1), // Bottom border
            ])
            .split(palette_area);

        // Render input line with cursor and Esc hint
        let esc_hint = "  Esc to close";
        let available_width = width as usize - 4 - esc_hint.len(); // Account for "> " prefix and hint

        let input_display = if self.query.is_empty() {
            vec![
                Span::styled(" > ", Style::default().fg(theme::accent())),
                Span::styled("Type to search...", Style::default().fg(theme::text_muted())),
                Span::styled(
                    format!("{:>width$}", esc_hint, width = available_width + esc_hint.len() - 17),
                    Style::default().fg(theme::text_muted()),
                ),
            ]
        } else {
            let (before, after) = self.query.split_at(self.cursor);
            let query_len = before.len() + after.len();
            let padding = available_width.saturating_sub(query_len);
            vec![
                Span::styled(" > ", Style::default().fg(theme::accent())),
                Span::styled(before.to_string(), Style::default().fg(theme::text())),
                Span::styled("│", Style::default().fg(theme::accent())), // Cursor
                Span::styled(after.to_string(), Style::default().fg(theme::text())),
                Span::styled(
                    format!("{:>width$}", esc_hint, width = padding + esc_hint.len()),
                    Style::default().fg(theme::text_muted()),
                ),
            ]
        };

        let input_line = Paragraph::new(Line::from(input_display)).style(Style::default().bg(theme::bg_surface()));
        frame.render_widget(input_line, chunks[0]);

        // Render filtered results
        let visible_start = if self.selected >= max_visible_items { self.selected - max_visible_items + 1 } else { 0 };

        let mut result_lines = Vec::new();
        for (i, cmd) in self.filtered_commands.iter().enumerate().skip(visible_start).take(max_visible_items) {
            let is_selected = i == self.selected;
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

            // Pad to full width for consistent highlight
            let content_len = prefix.len() + cmd.label.len() + 2 + cmd.description.len();
            let padding = (width as usize).saturating_sub(content_len);

            result_lines.push(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(&cmd.label, style),
                Span::styled(format!("  {}", cmd.description), desc_style),
                Span::styled(
                    " ".repeat(padding),
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
        frame.render_widget(results, chunks[1]);

        // Bottom border
        let border_line = "─".repeat(width as usize);
        let border = Paragraph::new(Line::from(Span::styled(border_line, Style::default().fg(theme::border()))))
            .style(Style::default().bg(theme::bg_surface()));
        frame.render_widget(border, chunks[2]);
    }
}
