use super::theme;
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
                        chars.next();
                        break;
                    }
                    width += 1;
                    chars.next();
                }
            }
            '*' | '_' => {
                // Check for double (bold) or single (italic)
                if chars.peek() == Some(&c) {
                    chars.next(); // consume second marker
                    // Count until closing **
                    #[allow(clippy::while_let_on_iterator)]
                    while let Some(next) = chars.next() {
                        if next == c && chars.peek() == Some(&c) {
                            chars.next();
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
                    chars.next(); // consume (
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

/// Render a markdown table with aligned columns
pub fn render_markdown_table(lines: &[&str], _base_style: Style) -> Vec<Vec<Span<'static>>> {
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
            // Render data row
            let mut spans: Vec<Span<'static>> = Vec::new();
            let is_header = row_idx == 0;

            for (col, width) in col_widths.iter().enumerate() {
                if col > 0 {
                    spans.push(Span::styled(" │ ", Style::default().fg(theme::border())));
                }

                let cell = row.get(col).map(|s| s.as_str()).unwrap_or("");
                let display_width = markdown_display_width(cell);
                let padding_needed = width.saturating_sub(display_width);

                if is_header {
                    // Headers: bold, no inline markdown parsing
                    spans.push(Span::styled(cell.to_string(), Style::default().fg(theme::accent()).bold()));
                } else {
                    // Data cells: parse inline markdown
                    let cell_spans = parse_inline_markdown(cell);
                    spans.extend(cell_spans);
                }

                // Add padding
                if padding_needed > 0 {
                    spans.push(Span::styled(" ".repeat(padding_needed), Style::default()));
                }
            }
            result.push(spans);
        }
    }

    result
}

/// Parse markdown text and return styled spans
pub fn parse_markdown_line(line: &str, base_style: Style) -> Vec<Span<'static>> {
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
pub fn parse_inline_markdown(text: &str) -> Vec<Span<'static>> {
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
                        chars.next();
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
                    chars.next(); // consume second */_

                    if !current.is_empty() {
                        spans.push(Span::styled(std::mem::take(&mut current), Style::default().fg(theme::text())));
                    }

                    // Bold text
                    let mut bold_text = String::new();
                    #[allow(clippy::while_let_on_iterator)]
                    while let Some(next) = chars.next() {
                        if next == c && chars.peek() == Some(&c) {
                            chars.next(); // consume closing **
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
                    chars.next(); // consume (
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
