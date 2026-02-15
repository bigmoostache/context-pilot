//! Shared UI helpers for panel rendering.
//!
//! Provides Cell, Align, and render_table so that extracted module crates
//! can render tables without depending on the main binary.

use ratatui::prelude::*;

use crate::constants::theme;

/// Column alignment for table cells
#[derive(Clone, Copy, Default)]
pub enum Align {
    #[default]
    Left,
    Right,
}

/// A single table cell with text, style, and alignment
pub struct Cell {
    pub text: String,
    pub style: Style,
    pub align: Align,
}

impl Cell {
    pub fn new(text: impl Into<String>, style: Style) -> Self {
        Self { text: text.into(), style, align: Align::Left }
    }
    pub fn right(text: impl Into<String>, style: Style) -> Self {
        Self { text: text.into(), style, align: Align::Right }
    }
}

/// Render a table with Unicode box-drawing separators.
///
/// - `header`: column headers (bold, accent-colored)
/// - `rows`: data rows as `Vec<Vec<Cell>>`
/// - `footer`: optional footer row (rendered bold, preceded by a separator)
/// - `indent`: number of leading spaces before each row
///
/// Returns `Vec<Line>` with aligned columns using ` │ ` separators and `─┼─` header underline.
pub fn render_table<'a>(header: &[Cell], rows: &[Vec<Cell>], footer: Option<&[Cell]>, indent: usize) -> Vec<Line<'a>> {
    let num_cols = header.len();

    // Compute column widths from header + all rows + footer
    let mut col_widths: Vec<usize> = header.iter().map(|c| c.text.chars().count()).collect();
    col_widths.resize(num_cols, 0);

    for row in rows {
        for (col, cell) in row.iter().enumerate() {
            if col < num_cols {
                col_widths[col] = col_widths[col].max(cell.text.chars().count());
            }
        }
    }
    if let Some(f) = footer {
        for (col, cell) in f.iter().enumerate() {
            if col < num_cols {
                col_widths[col] = col_widths[col].max(cell.text.chars().count());
            }
        }
    }

    let pad = " ".repeat(indent);
    let mut lines: Vec<Line> = Vec::new();

    let separator = || -> Line<'static> {
        let mut spans: Vec<Span<'static>> = vec![Span::raw(pad.clone())];
        for (col, width) in col_widths.iter().enumerate() {
            if col > 0 {
                spans.push(Span::styled("─┼─", Style::default().fg(theme::border())));
            }
            spans.push(Span::styled("─".repeat(*width), Style::default().fg(theme::border())));
        }
        Line::from(spans)
    };

    let render_row = |cells: &[Cell], bold: bool| -> Line<'static> {
        let mut spans: Vec<Span<'static>> = vec![Span::raw(pad.clone())];
        for (col, col_w) in col_widths.iter().enumerate().take(num_cols) {
            if col > 0 {
                spans.push(Span::styled(" │ ", Style::default().fg(theme::border())));
            }
            if let Some(cell) = cells.get(col) {
                let padded = match cell.align {
                    Align::Right => format!("{:>width$}", cell.text, width = *col_w),
                    Align::Left => format!("{:<width$}", cell.text, width = *col_w),
                };
                let style = if bold { cell.style.bold() } else { cell.style };
                spans.push(Span::styled(padded, style));
            } else {
                spans.push(Span::styled(" ".repeat(*col_w), Style::default()));
            }
        }
        Line::from(spans)
    };

    // Header row (bold accent)
    {
        let mut spans: Vec<Span<'static>> = vec![Span::raw(pad.clone())];
        for (col, hdr) in header.iter().enumerate() {
            if col > 0 {
                spans.push(Span::styled(" │ ", Style::default().fg(theme::border())));
            }
            let w = col_widths[col];
            let padded = match hdr.align {
                Align::Right => format!("{:>width$}", hdr.text, width = w),
                Align::Left => format!("{:<width$}", hdr.text, width = w),
            };
            spans.push(Span::styled(padded, Style::default().fg(theme::accent()).bold()));
        }
        lines.push(Line::from(spans));
    }

    // Header separator
    lines.push(separator());

    // Data rows
    for row in rows {
        lines.push(render_row(row, false));
    }

    // Footer (separator + bold row)
    if let Some(f) = footer {
        lines.push(separator());
        lines.push(render_row(f, true));
    }

    lines
}

/// Find size pattern in tree output (e.g., "123K" at end of line)
pub fn find_size_pattern(line: &str) -> Option<usize> {
    let trimmed = line.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    let last_char = trimmed.chars().last()?;
    if !matches!(last_char, 'B' | 'K' | 'M') {
        return None;
    }
    let bytes = trimmed.as_bytes();
    let mut num_start = bytes.len() - 1;
    while num_start > 0 && bytes[num_start - 1].is_ascii_digit() {
        num_start -= 1;
    }
    if num_start > 0 && bytes[num_start - 1] == b' ' { Some(num_start - 1) } else { None }
}

/// Find children count pattern in tree output (e.g., "(5 children)" or "(1 child)")
/// Returns (start_index, end_index) of the pattern
pub fn find_children_pattern(line: &str) -> Option<(usize, usize)> {
    if let Some(start) = line.find(" (") {
        let rest = &line[start + 2..];
        if let Some(end_paren) = rest.find(')') {
            let inner = &rest[..end_paren];
            if inner.ends_with(" child") || inner.ends_with(" children") {
                let num_part = inner.split_whitespace().next()?;
                if num_part.parse::<usize>().is_ok() {
                    return Some((start + 1, start + 2 + end_paren + 1));
                }
            }
        }
    }
    None
}
