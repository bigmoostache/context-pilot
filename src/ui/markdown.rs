use super::theme;
use cp_base::cast::SafeCast;
use ratatui::prelude::*;

/// Calculate the display width of text after stripping markdown markers
fn markdown_display_width(text: &str) -> usize {
    let mut width = 0;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '`' => {
                // Skip to closing backtick, count content
                while let Some(&next) = chars.peek() {
                    if next == '`' {
                        let _r = chars.next();
                        break;
                    }
                    width += 1;
                    let _r = chars.next();
                }
            }
            '*' | '_' => {
                // Check for double (bold) or single (italic)
                if chars.peek() == Some(&c) {
                    let _r = chars.next(); // consume second marker
                    // Count until closing **
                    while let Some(next) = chars.next() {
                        if next == c && chars.peek() == Some(&c) {
                            let _r = chars.next();
                            break;
                        }
                        width += 1;
                    }
                } else {
                    // Single marker (italic) - count until closing
                    for next in chars.by_ref() {
                        if next == c {
                            break;
                        }
                        width += 1;
                    }
                }
            }
            '[' => {
                // Link [text](url) - only count the text part
                let mut link_text_len = 0;
                let mut found_bracket = false;
                for next in chars.by_ref() {
                    if next == ']' {
                        found_bracket = true;
                        break;
                    }
                    link_text_len += 1;
                }
                if found_bracket && chars.peek() == Some(&'(') {
                    let _r = chars.next(); // consume (
                    for next in chars.by_ref() {
                        if next == ')' {
                            break;
                        }
                    }
                    width += link_text_len;
                } else {
                    // Not a valid link
                    width += 1 + link_text_len;
                    if found_bracket {
                        width += 1;
                    }
                }
            }
            _ => {
                width += 1;
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

    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;

    for word in text.split_whitespace() {
        let word_width = markdown_display_width(word);

        if current_width == 0 {
            // First word on line — always add it (even if longer than width)
            if word_width > width {
                // Break long word character by character
                for ch in word.chars() {
                    if current_width >= width {
                        lines.push(std::mem::take(&mut current_line));
                        current_width = 0;
                    }
                    current_line.push(ch);
                    current_width += 1;
                }
            } else {
                current_line.push_str(word);
                current_width = word_width;
            }
        } else if current_width + 1 + word_width <= width {
            // Fits on current line with a space
            current_line.push(' ');
            current_line.push_str(word);
            current_width += 1 + word_width;
        } else {
            // Doesn't fit — start a new line
            lines.push(std::mem::take(&mut current_line));
            if word_width > width {
                current_width = 0;
                for ch in word.chars() {
                    if current_width >= width {
                        lines.push(std::mem::take(&mut current_line));
                        current_width = 0;
                    }
                    current_line.push(ch);
                    current_width += 1;
                }
            } else {
                current_line.push_str(word);
                current_width = word_width;
            }
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

/// Render a markdown table with aligned columns.
///
/// Strategy: compute fixed column widths → for each row, wrap cell text to fit
/// → render each display line as a sequence of fixed-width cells separated by │.
/// Vertical separators are always at the same character positions.
pub(crate) fn render_markdown_table(lines: &[&str], _base_style: Style, max_width: usize) -> Vec<Vec<Span<'static>>> {
    // Parse all rows into cells
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut is_separator_row: Vec<bool> = Vec::new();

    for line in lines {
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
    let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut col_widths: Vec<usize> = vec![0; num_cols];

    for (i, row) in rows.iter().enumerate() {
        if is_separator_row[i] {
            continue; // Don't count separator row for width calculation
        }
        for (col, cell) in row.iter().enumerate() {
            if col < col_widths.len() {
                col_widths[col] = col_widths[col].max(markdown_display_width(cell));
            }
        }
    }

    // Constrain columns to fit within max_width
    // Each column separator " │ " takes 3 chars, so separators = (num_cols - 1) * 3
    let separator_width = if num_cols > 1 { (num_cols - 1) * 3 } else { 0 };
    let total_content_width: usize = col_widths.iter().sum();
    let total_width = total_content_width + separator_width;

    if total_width > max_width && max_width > separator_width {
        let available = max_width - separator_width;
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
            let mut remaining = available - used;
            // Sort column indices by original width (descending) to give extra space to wider columns
            let mut col_indices: Vec<usize> = (0..num_cols).collect();
            col_indices.sort_by(|&a, &b| col_widths[b].cmp(&col_widths[a]));
            for &idx in &col_indices {
                if remaining == 0 {
                    break;
                }
                new_widths[idx] += 1;
                remaining -= 1;
            }
        }
        col_widths = new_widths;
    }

    // Render each row with aligned columns
    let mut result: Vec<Vec<Span<'static>>> = Vec::new();

    for (row_idx, row) in rows.iter().enumerate() {
        if is_separator_row[row_idx] {
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

                    let cell_text =
                        wrapped_cells.get(col).and_then(|lines| lines.get(line_idx)).map_or("", |s| s.as_str());

                    // Build a single fixed-width span for this cell.
                    // Content + padding always equals exactly `width` display chars.
                    // This guarantees │ separators are at fixed positions.
                    let display_width = markdown_display_width(cell_text);
                    let padding = " ".repeat(width.saturating_sub(display_width));

                    if is_header {
                        // Header: single styled span with content + padding baked in
                        spans.push(Span::styled(
                            format!("{}{}", cell_text, padding),
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
            if row_idx < rows.len() - 1 && !is_separator_row.get(row_idx + 1).copied().unwrap_or(false) {
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

/// Parse markdown text and return styled spans
pub(crate) fn parse_markdown_line(line: &str, base_style: Style) -> Vec<Span<'static>> {
    let trimmed = line.trim_start();

    // Headers: # ## ### etc.
    if trimmed.starts_with('#') {
        let level = trimmed.chars().take_while(|&c| c == '#').count();
        let content = trimmed[level..].trim_start();

        let style = match level {
            1 => Style::default().fg(theme::accent()).bold(),
            2 => Style::default().fg(theme::accent()),
            3 => Style::default().fg(theme::accent()).italic(),
            _ => Style::default().fg(theme::text_secondary()).italic(),
        };

        return vec![Span::styled(content.to_string(), style)];
    }

    // Bullet points: - or *
    if let Some(stripped) = trimmed.strip_prefix("- ") {
        let content = stripped.to_string();
        let indent = line.len() - trimmed.len();
        let mut spans = vec![
            Span::styled(" ".repeat(indent), base_style),
            Span::styled("• ", Style::default().fg(theme::accent_dim())),
        ];
        spans.extend(parse_inline_markdown(&content));
        return spans;
    }

    if trimmed.starts_with("* ") && !trimmed.starts_with("**") {
        let content = trimmed[2..].to_string();
        let indent = line.len() - trimmed.len();
        let mut spans = vec![
            Span::styled(" ".repeat(indent), base_style),
            Span::styled("• ", Style::default().fg(theme::accent_dim())),
        ];
        spans.extend(parse_inline_markdown(&content));
        return spans;
    }

    // Regular line - parse inline markdown
    parse_inline_markdown(line)
}

/// Parse inline markdown (bold, italic, code)
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
                        let _r = chars.next();
                        break;
                    }
                    code.push(chars.next().unwrap());
                }

                if !code.is_empty() {
                    spans.push(Span::styled(code, Style::default().fg(theme::warning())));
                }
            }
            '*' | '_' => {
                // Check for bold (**) or italic (*)
                let is_double = chars.peek() == Some(&c);

                if is_double {
                    let _r = chars.next(); // consume second */_

                    if !current.is_empty() {
                        spans.push(Span::styled(std::mem::take(&mut current), Style::default().fg(theme::text())));
                    }

                    // Bold text
                    let mut bold_text = String::new();
                    while let Some(next) = chars.next() {
                        if next == c && chars.peek() == Some(&c) {
                            let _r = chars.next(); // consume closing **
                            break;
                        }
                        bold_text.push(next);
                    }

                    if !bold_text.is_empty() {
                        spans.push(Span::styled(bold_text, Style::default().fg(theme::text()).bold()));
                    }
                } else {
                    // Italic text - look for closing marker
                    if !current.is_empty() {
                        spans.push(Span::styled(std::mem::take(&mut current), Style::default().fg(theme::text())));
                    }

                    let mut italic_text = String::new();
                    let mut found_close = false;
                    for next in chars.by_ref() {
                        if next == c {
                            found_close = true;
                            break;
                        }
                        italic_text.push(next);
                    }

                    if found_close && !italic_text.is_empty() {
                        spans.push(Span::styled(italic_text, Style::default().fg(theme::text()).italic()));
                    } else {
                        // Not actually italic, restore
                        current.push(c);
                        current.push_str(&italic_text);
                    }
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
                    let _r = chars.next(); // consume (
                    let mut url = String::new();
                    for next in chars.by_ref() {
                        if next == ')' {
                            break;
                        }
                        url.push(next);
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
