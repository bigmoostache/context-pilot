use unicode_width::UnicodeWidthStr;

pub fn truncate_string(s: &str, max_width: usize) -> String {
    if s.width() <= max_width {
        s.to_string()
    } else {
        let mut result = String::new();
        let mut width = 0;
        for c in s.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
            if width + cw + 1 > max_width {
                result.push('…');
                break;
            }
            result.push(c);
            width += cw;
        }
        result
    }
}

pub fn format_number(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Format a millisecond delta as a human-readable "x ago" string
pub fn format_time_ago(delta_ms: u64) -> String {
    let seconds = delta_ms / 1000;
    if seconds < 60 {
        format!("{}s ago", seconds)
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else {
        format!("{}h ago", seconds / 3600)
    }
}

/// Word-wrap text to fit within a given width
pub fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;

    for word in text.split_whitespace() {
        let word_width = word.chars().count();

        if current_width == 0 {
            // First word on line
            current_line = word.to_string();
            current_width = word_width;
        } else if current_width + 1 + word_width <= max_width {
            // Word fits on current line
            current_line.push(' ');
            current_line.push_str(word);
            current_width += 1 + word_width;
        } else {
            // Word doesn't fit, start new line
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

/// Count how many lines a Line will take when wrapped to a given width
/// Uses unicode width for accurate display width calculation
pub fn count_wrapped_lines(line: &ratatui::prelude::Line, max_width: usize) -> usize {
    use unicode_width::UnicodeWidthStr;

    if max_width == 0 {
        return 1;
    }

    // Concatenate all span content
    let full_text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    if full_text.is_empty() {
        return 1;
    }

    // Simulate word wrapping
    let mut line_count = 1;
    let mut current_width = 0;

    for word in full_text.split_inclusive(|c: char| c.is_whitespace()) {
        let word_width = word.width();

        if current_width == 0 {
            current_width = word_width;
        } else if current_width + word_width <= max_width {
            current_width += word_width;
        } else {
            // Word doesn't fit, start new line
            line_count += 1;
            current_width = word_width;
        }

        // Handle very long words that need to be broken
        while current_width > max_width {
            line_count += 1;
            current_width = current_width.saturating_sub(max_width);
        }
    }

    line_count
}

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
    pub style: ratatui::prelude::Style,
    pub align: Align,
}

impl Cell {
    pub fn new(text: impl Into<String>, style: ratatui::prelude::Style) -> Self {
        Self { text: text.into(), style, align: Align::Left }
    }
    pub fn right(text: impl Into<String>, style: ratatui::prelude::Style) -> Self {
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
pub fn render_table<'a>(
    header: &[Cell],
    rows: &[Vec<Cell>],
    footer: Option<&[Cell]>,
    indent: usize,
) -> Vec<ratatui::prelude::Line<'a>> {
    use crate::ui::theme;
    use ratatui::prelude::*;

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
    // Look for patterns like "(N children)" or "(1 child)"
    if let Some(start) = line.find(" (") {
        let rest = &line[start + 2..];
        if let Some(end_paren) = rest.find(')') {
            let inner = &rest[..end_paren];
            // Check if it matches "N child" or "N children"
            if inner.ends_with(" child") || inner.ends_with(" children") {
                // Verify the first part is a number
                let num_part = inner.split_whitespace().next()?;
                if num_part.parse::<usize>().is_ok() {
                    return Some((start + 1, start + 2 + end_paren + 1));
                }
            }
        }
    }
    None
}
