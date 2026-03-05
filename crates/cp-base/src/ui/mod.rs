//! Shared UI helpers for panel rendering.
//!
//! Provides Cell, Align, and render_table so that extracted module crates
//! can render tables without depending on the main binary.

use ratatui::prelude::{Line, Span, Style};
use unicode_width::UnicodeWidthStr;

use crate::config::accessors::theme;

/// Interactive question form types (ask_user_question tool).
pub mod question_form;
/// Render cache types for conversation panel performance.
pub mod render_cache;

/// Column alignment for table cells
#[derive(Debug, Clone, Copy, Default)]
pub enum Align {
    #[default]
    /// Align text to the left, padding with trailing spaces.
    Left,
    /// Align text to the right, padding with leading spaces.
    Right,
}

/// A single table cell with text, style, and alignment
#[derive(Debug)]
pub struct Cell {
    /// Display text content.
    pub text: String,
    /// Ratatui style (fg/bg/modifiers).
    pub style: Style,
    /// Column alignment.
    pub align: Align,
}

impl Cell {
    /// Create a left-aligned cell with the given text and style.
    pub fn new<T: Into<String>>(text: T, style: Style) -> Self {
        Self { text: text.into(), style, align: Align::Left }
    }
    /// Create a right-aligned cell with the given text and style.
    pub fn right<T: Into<String>>(text: T, style: Style) -> Self {
        Self { text: text.into(), style, align: Align::Right }
    }
}

/// Pad a string to a target display width using spaces, respecting alignment.
fn pad_to_width(text: &str, target: usize, align: Align) -> String {
    let w = UnicodeWidthStr::width(text);
    let deficit = target.saturating_sub(w);
    match align {
        Align::Left => format!("{}{}", text, " ".repeat(deficit)),
        Align::Right => format!("{}{}", " ".repeat(deficit), text),
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
#[must_use]
pub fn render_table(header: &[Cell], rows: &[Vec<Cell>], footer: Option<&[Cell]>, indent: usize) -> Vec<Line<'static>> {
    let num_cols = header.len();

    // Compute column widths from header + all rows + footer using display width
    let mut col_widths: Vec<usize> = header.iter().map(|c| UnicodeWidthStr::width(c.text.as_str())).collect();
    col_widths.resize(num_cols, 0);

    for row in rows {
        for (col, cell) in row.iter().enumerate() {
            if let Some(w) = col_widths.get_mut(col) {
                *w = (*w).max(UnicodeWidthStr::width(cell.text.as_str()));
            }
        }
    }
    if let Some(f) = footer {
        for (col, cell) in f.iter().enumerate() {
            if let Some(w) = col_widths.get_mut(col) {
                *w = (*w).max(UnicodeWidthStr::width(cell.text.as_str()));
            }
        }
    }

    let pad = " ".repeat(indent);
    let mut lines: Vec<Line<'_>> = Vec::new();

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
                let padded = pad_to_width(&cell.text, *col_w, cell.align);
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
            let w = col_widths.get(col).copied().unwrap_or(0);
            let padded = pad_to_width(&hdr.text, w, hdr.align);
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

/// Simple text-cell for `render_table_text`. Style-free, just text + alignment.
#[derive(Debug)]
pub struct TextCell {
    /// Display text content.
    pub text: String,
    /// Column alignment.
    pub align: Align,
}

impl TextCell {
    /// Create a left-aligned text cell.
    pub fn left<T: Into<String>>(text: T) -> Self {
        Self { text: text.into(), align: Align::Left }
    }
    /// Create a right-aligned text cell.
    pub fn right<T: Into<String>>(text: T) -> Self {
        Self { text: text.into(), align: Align::Right }
    }
}

/// Render a table as a plain-text string for LLM context.
///
/// Uses ` │ ` column separators and `─┼─` header underline.
/// Column widths computed via `UnicodeWidthStr` for correct alignment.
///
/// Example output:
/// ```text
/// ID  │ Summary          │ Importance │ Labels
/// ────┼──────────────────┼────────────┼──────────
/// M1  │ Some memory note │ high       │ arch, bug
/// ```
/// ```
#[must_use]
pub fn render_table_text(header: &[&str], rows: &[Vec<TextCell>]) -> String {
    let num_cols = header.len();

    // Compute column widths using display width
    let mut col_widths: Vec<usize> = header.iter().map(|h| UnicodeWidthStr::width(*h)).collect();
    col_widths.resize(num_cols, 0);

    for row in rows {
        for (col, cell) in row.iter().enumerate() {
            if let Some(w) = col_widths.get_mut(col) {
                *w = (*w).max(UnicodeWidthStr::width(cell.text.as_str()));
            }
        }
    }

    let mut output = String::new();

    // Helper to pad text to target display width
    let pad = |text: &str, target: usize, align: Align| -> String {
        let w = UnicodeWidthStr::width(text);
        let deficit = target.saturating_sub(w);
        match align {
            Align::Left => format!("{}{}", text, " ".repeat(deficit)),
            Align::Right => format!("{}{}", " ".repeat(deficit), text),
        }
    };

    // Header
    for (col, hdr) in header.iter().enumerate() {
        if col > 0 {
            output.push_str(" │ ");
        }
        output.push_str(&pad(hdr, col_widths.get(col).copied().unwrap_or(0), Align::Left));
    }
    output.push('\n');

    // Separator
    for (col, width) in col_widths.iter().enumerate() {
        if col > 0 {
            output.push_str("─┼─");
        }
        output.push_str(&"─".repeat(*width));
    }
    output.push('\n');

    // Rows
    for row in rows {
        for (col, col_w) in col_widths.iter().enumerate().take(num_cols) {
            if col > 0 {
                output.push_str(" │ ");
            }
            if let Some(cell) = row.get(col) {
                output.push_str(&pad(&cell.text, *col_w, cell.align));
            } else {
                output.push_str(&" ".repeat(*col_w));
            }
        }
        output.push('\n');
    }

    output
}

/// Find size pattern in tree output (e.g., "123K" at end of line)
#[must_use]
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
    let mut num_start = bytes.len().saturating_sub(1);
    while num_start > 0 && bytes.get(num_start.saturating_sub(1)).is_some_and(u8::is_ascii_digit) {
        num_start = num_start.saturating_sub(1);
    }
    (num_start > 0 && bytes.get(num_start.saturating_sub(1)).copied() == Some(b' '))
        .then(|| num_start.saturating_sub(1))
}

/// Find children count pattern in tree output (e.g., "(5 children)" or "(1 child)")
/// Returns (`start_index`, `end_index`) of the pattern
#[must_use]
pub fn find_children_pattern(line: &str) -> Option<(usize, usize)> {
    if let Some(start) = line.find(" (") {
        let rest = line.get(start.saturating_add(2)..).unwrap_or("");
        if let Some(end_paren) = rest.find(')') {
            let inner = rest.get(..end_paren).unwrap_or("");
            if inner.ends_with(" child") || inner.ends_with(" children") {
                let num_part = inner.split_whitespace().next()?;
                if num_part.parse::<usize>().is_ok() {
                    return Some((
                        start.saturating_add(1),
                        start.saturating_add(2).saturating_add(end_paren).saturating_add(1),
                    ));
                }
            }
        }
    }
    None
}
