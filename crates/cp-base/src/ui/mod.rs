//! Shared UI helpers for panel rendering.
//!
//! Provides plain-text table rendering for LLM context output and
//! utility functions for tree output parsing.

use unicode_width::UnicodeWidthStr;

/// Render cache types for conversation panel performance.
pub mod render_cache;

/// Column alignment for table cells.
#[derive(Debug, Clone, Copy, Default)]
pub enum Align {
    #[default]
    /// Align text to the left, padding with trailing spaces.
    Left,
    /// Align text to the right, padding with leading spaces.
    Right,
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
    pub fn left<T>(text: T) -> Self where T: Into<String> {
        Self { text: text.into(), align: Align::Left }
    }
    /// Create a right-aligned text cell.
    pub fn right<T>(text: T) -> Self where T: Into<String> {
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
