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
    let full_text: String = line.spans.iter()
        .map(|s| s.content.as_ref())
        .collect();

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

    if num_start > 0 && bytes[num_start - 1] == b' ' {
        Some(num_start - 1)
    } else {
        None
    }
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
