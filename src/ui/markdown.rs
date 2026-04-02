use super::theme;
use cp_base::cast::Safe as _;
use ratatui::prelude::{Span, Style};

/// Calculate the display width of text after stripping markdown markers.
fn markdown_display_width(text: &str) -> usize {
    let mut width = 0usize;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '`' => {
                // Skip to closing backtick, count content
                while let Some(&next) = chars.peek() {
                    if next == '`' {
                        let _r1 = chars.next();
                        break;
                    }
                    width = width.saturating_add(1);
                    let _r1 = chars.next();
                }
            }
            '*' | '_' => {
                // Check for double (bold) — single markers are treated as plain text
                if chars.peek() == Some(&c) {
                    let _r1 = chars.next(); // consume second marker
                    // Count until closing **/__
                    while let Some(next) = chars.next() {
                        if next == c && chars.peek() == Some(&c) {
                            let _r2 = chars.next();
                            break;
                        }
                        width = width.saturating_add(1);
                    }
                } else {
                    // Single * or _ — treat as literal character
                    width = width.saturating_add(1);
                }
            }
            '[' => {
                // Link [text](url) - only count the text part
                let mut link_text_len = 0usize;
                let mut found_bracket = false;
                for next in chars.by_ref() {
                    if next == ']' {
                        found_bracket = true;
                        break;
                    }
                    link_text_len = link_text_len.saturating_add(1);
                }
                if found_bracket && chars.peek() == Some(&'(') {
                    let _r1 = chars.next(); // consume (
                    for next in chars.by_ref() {
                        if next == ')' {
                            break;
                        }
                    }
                    width = width.saturating_add(link_text_len);
                } else {
                    // Not a valid link
                    width = width.saturating_add(1).saturating_add(link_text_len);
                    if found_bracket {
                        width = width.saturating_add(1);
                    }
                }
            }
            _ => {
                width = width.saturating_add(1);
            }
        }
    }

    width
}

/// Wrap text to fit within a given width, breaking on word boundaries.
/// Returns a Vec of lines, each fitting within `width` characters.
fn wrap_cell_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    if markdown_display_width(text) <= width {
        return vec![text.to_string()];
    }

    let mut result_lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0usize;

    for word in text.split_whitespace() {
        let word_width = markdown_display_width(word);

        if current_width == 0 {
            // First word on line — always add it (even if longer than width)
            if word_width > width {
                // Break long word character by character
                for ch in word.chars() {
                    if current_width >= width {
                        result_lines.push(std::mem::take(&mut current_line));
                        current_width = 0;
                    }
                    current_line.push(ch);
                    current_width = current_width.saturating_add(1);
                }
            } else {
                current_line.push_str(word);
                current_width = word_width;
            }
        } else if current_width.saturating_add(1).saturating_add(word_width) <= width {
            // Fits on current line with a space
            current_line.push(' ');
            current_line.push_str(word);
            current_width = current_width.saturating_add(1).saturating_add(word_width);
        } else {
            // Doesn't fit — start a new line
            result_lines.push(std::mem::take(&mut current_line));
            if word_width > width {
                current_width = 0;
                for ch in word.chars() {
                    if current_width >= width {
                        result_lines.push(std::mem::take(&mut current_line));
                        current_width = 0;
                    }
                    current_line.push(ch);
                    current_width = current_width.saturating_add(1);
                }
            } else {
                current_line.push_str(word);
                current_width = word_width;
            }
        }
    }

    if !current_line.is_empty() {
        result_lines.push(current_line);
    }

    if result_lines.is_empty() {
        result_lines.push(String::new());
    }

    result_lines
}

/// Render a markdown table with aligned columns.
///
/// Strategy: compute fixed column widths -> for each row, wrap cell text to fit
/// -> render each display line as a sequence of fixed-width cells separated by |.
/// Vertical separators are always at the same character positions.
pub(crate) fn render_markdown_table(
    table_lines: &[&str],
    _base_style: Style,
    max_width: usize,
) -> Vec<Vec<Span<'static>>> {
    // Parse all rows into cells
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut is_separator_row: Vec<bool> = Vec::new();

    for line in table_lines {
        let trimmed = line.trim();
        // Remove leading and trailing pipes
        let inner = trimmed.trim_start_matches('|').trim_end_matches('|');
        let cells: Vec<String> = inner.split('|').map(|c| c.trim().to_string()).collect();

        // Check if this is a separator row (contains only dashes and colons)
        let is_sep = cells.iter().all(|c| c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' '));

        is_separator_row.push(is_sep);
        rows.push(cells);
    }

    // Calculate max width for each column (using display width, not raw length)
    let num_cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut col_widths: Vec<usize> = vec![0; num_cols];

    for (i, row) in rows.iter().enumerate() {
        if is_separator_row.get(i).copied().unwrap_or(false) {
            continue; // Don't count separator row for width calculation
        }
        for (col, cell) in row.iter().enumerate() {
            if let Some(cw) = col_widths.get_mut(col) {
                *cw = (*cw).max(markdown_display_width(cell));
            }
        }
    }

    // Constrain columns to fit within max_width
    // Each column separator " │ " takes 3 chars, so separators = (num_cols - 1) * 3
    let separator_width = if num_cols > 1 { num_cols.saturating_sub(1).saturating_mul(3) } else { 0 };
    let total_content_width: usize = col_widths.iter().sum();
    let total_width = total_content_width.saturating_add(separator_width);

    if total_width > max_width && max_width > separator_width {
        let available = max_width.saturating_sub(separator_width);
        // Shrink columns proportionally
        let mut new_widths: Vec<usize> = col_widths
            .iter()
            .map(|&w| {
                let proportional = (w.to_f64() / total_content_width.to_f64() * available.to_f64()).to_usize();
                proportional.max(3) // minimum 3 chars per column
            })
            .collect();
        // Distribute any remaining space to the widest columns
        let used: usize = new_widths.iter().sum();
        if used < available {
            let mut remaining = available.saturating_sub(used);
            // Sort column indices by original width (descending) to give extra space to wider columns
            let mut col_indices: Vec<usize> = (0..num_cols).collect();
            col_indices.sort_by(|&a, &b| {
                let width_a = col_widths.get(a).copied().unwrap_or(0);
                let width_b = col_widths.get(b).copied().unwrap_or(0);
                width_b.cmp(&width_a)
            });
            for &idx in &col_indices {
                if remaining == 0 {
                    break;
                }
                if let Some(nw) = new_widths.get_mut(idx) {
                    *nw = nw.saturating_add(1);
                }
                remaining = remaining.saturating_sub(1);
            }
        }
        col_widths = new_widths;
    }

    // Render each row with aligned columns
    let mut result: Vec<Vec<Span<'static>>> = Vec::new();

    for (row_idx, row) in rows.iter().enumerate() {
        if is_separator_row.get(row_idx).copied().unwrap_or(false) {
            // Render separator row with dashes
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (col, width) in col_widths.iter().enumerate() {
                if col > 0 {
                    spans.push(Span::styled("─┼─", Style::default().fg(theme::border())));
                }
                spans.push(Span::styled("─".repeat(*width), Style::default().fg(theme::border())));
            }
            result.push(spans);
        } else {
            // Render data row (with multi-line wrapping)
            let is_header = row_idx == 0;

            // Wrap each cell's content to its column width
            let mut wrapped_cells: Vec<Vec<String>> = Vec::new();
            let mut max_lines = 1usize;

            for (col, width) in col_widths.iter().enumerate() {
                let cell = row.get(col).map_or("", |s| s.as_str());
                let cell_lines = wrap_cell_text(cell, *width);
                max_lines = max_lines.max(cell_lines.len());
                wrapped_cells.push(cell_lines);
            }

            // Render each display line of this logical row
            for line_idx in 0..max_lines {
                let mut spans: Vec<Span<'static>> = Vec::new();

                for (col, width) in col_widths.iter().enumerate() {
                    if col > 0 {
                        spans.push(Span::styled(" │ ", Style::default().fg(theme::border())));
                    }

                    let cell_text = wrapped_cells
                        .get(col)
                        .and_then(|cell_lines| cell_lines.get(line_idx))
                        .map_or("", |s| s.as_str());

                    // Build a single fixed-width span for this cell.
                    // Content + padding always equals exactly `width` display chars.
                    // This guarantees │ separators are at fixed positions.
                    let display_width = markdown_display_width(cell_text);
                    let padding = " ".repeat(width.saturating_sub(display_width));

                    if is_header {
                        // Header: single styled span with content + padding baked in
                        spans.push(Span::styled(
                            format!("{cell_text}{padding}"),
                            Style::default().fg(theme::accent()).bold(),
                        ));
                    } else if cell_text.is_empty() {
                        // Empty cell: just spaces
                        spans.push(Span::styled(" ".repeat(*width), Style::default()));
                    } else {
                        // Data cell: render markdown, then add padding as a plain span
                        let cell_spans = parse_inline_markdown(cell_text);
                        spans.extend(cell_spans);
                        if !padding.is_empty() {
                            spans.push(Span::styled(padding, Style::default()));
                        }
                    }
                }
                result.push(spans);
            }

            // Add thin separator line between data rows (not after last row, not after header's separator)
            let next_row_idx = row_idx.saturating_add(1);
            if next_row_idx < rows.len() && !is_separator_row.get(next_row_idx).copied().unwrap_or(false) {
                let mut sep_spans: Vec<Span<'static>> = Vec::new();
                for (col, width) in col_widths.iter().enumerate() {
                    if col > 0 {
                        sep_spans.push(Span::styled("─┼─", Style::default().fg(theme::border())));
                    }
                    sep_spans.push(Span::styled("─".repeat(*width), Style::default().fg(theme::border())));
                }
                result.push(sep_spans);
            }
        }
    }

    result
}

/// Parse inline markdown (bold, italic, code) and return styled spans.
pub(crate) fn parse_inline_markdown(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut chars = text.chars().peekable();
    let mut current = String::new();

    while let Some(c) = chars.next() {
        match c {
            '`' => {
                // Inline code
                if !current.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut current), Style::default().fg(theme::text())));
                }

                let mut code = String::new();
                while let Some(&next) = chars.peek() {
                    if next == '`' {
                        let _r1 = chars.next();
                        break;
                    }
                    if let Some(ch) = chars.next() {
                        code.push(ch);
                    }
                }

                if !code.is_empty() {
                    spans.push(Span::styled(code, Style::default().fg(theme::warning())));
                }
            }
            '*' | '_' => {
                // Check for bold (**/__) — single markers are plain text
                let is_double = chars.peek() == Some(&c);

                if is_double {
                    let _r1 = chars.next(); // consume second */_

                    if !current.is_empty() {
                        spans.push(Span::styled(std::mem::take(&mut current), Style::default().fg(theme::text())));
                    }

                    // Bold text
                    let mut bold_text = String::new();
                    while let Some(next) = chars.next() {
                        if next == c && chars.peek() == Some(&c) {
                            let _r2 = chars.next(); // consume closing **
                            break;
                        }
                        bold_text.push(next);
                    }

                    if !bold_text.is_empty() {
                        spans.push(Span::styled(bold_text, Style::default().fg(theme::text()).bold()));
                    }
                } else {
                    // Single * or _ — treat as literal character
                    current.push(c);
                }
            }
            '[' => {
                // Possible link [text](url)
                let mut link_text = String::new();
                let mut found_bracket = false;

                for next in chars.by_ref() {
                    if next == ']' {
                        found_bracket = true;
                        break;
                    }
                    link_text.push(next);
                }

                if found_bracket && chars.peek() == Some(&'(') {
                    let _r1 = chars.next(); // consume (
                    // Skip URL characters — we only display the link text
                    for next in chars.by_ref() {
                        if next == ')' {
                            break;
                        }
                    }

                    // Display link text in accent color
                    if !current.is_empty() {
                        spans.push(Span::styled(std::mem::take(&mut current), Style::default().fg(theme::text())));
                    }
                    spans.push(Span::styled(link_text, Style::default().fg(theme::accent()).underlined()));
                } else {
                    // Not a valid link, restore
                    current.push('[');
                    current.push_str(&link_text);
                    if found_bracket {
                        current.push(']');
                    }
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    if !current.is_empty() {
        spans.push(Span::styled(current, Style::default().fg(theme::text())));
    }

    if spans.is_empty() {
        spans.push(Span::styled("", Style::default()));
    }

    spans
}
