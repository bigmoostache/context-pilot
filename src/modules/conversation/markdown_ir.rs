/// IR-native markdown parser: returns `Vec<cp_render::Span>` instead of ratatui spans.
///
/// Mirrors `ui::markdown::parse_markdown_line` and `parse_inline_markdown`
/// but outputs platform-agnostic IR types.
use cp_render::{Semantic, Span};

/// Parse a markdown line and return IR spans.
///
/// Handles headers (`#`), bullet points (`- `, `* `), and inline formatting.
pub(super) fn parse_markdown_line_ir(line: &str) -> Vec<Span> {
    let trimmed = line.trim_start();

    // Headers: # ## ### etc.
    if trimmed.starts_with('#') {
        let level = trimmed.chars().take_while(|&c| c == '#').count();
        let content = trimmed.get(level..).unwrap_or("").trim_start();

        let semantic = match level {
            1..=3 => Semantic::Accent,
            _ => Semantic::Code,
        };

        return if level <= 1 {
            vec![Span::styled(content.to_owned(), semantic).bold()]
        } else {
            vec![Span::styled(content.to_owned(), semantic)]
        };
    }

    // Bullet points: - or *
    if let Some(stripped) = trimmed.strip_prefix("- ") {
        let indent = line.len().saturating_sub(trimmed.len());
        let mut spans = vec![Span::new(" ".repeat(indent)), Span::styled("• ".to_owned(), Semantic::AccentDim)];
        spans.extend(parse_inline_markdown_ir(stripped));
        return spans;
    }

    if trimmed.starts_with("* ") && !trimmed.starts_with("**") {
        let content = trimmed.get(2..).unwrap_or("");
        let indent = line.len().saturating_sub(trimmed.len());
        let mut spans = vec![Span::new(" ".repeat(indent)), Span::styled("• ".to_owned(), Semantic::AccentDim)];
        spans.extend(parse_inline_markdown_ir(content));
        return spans;
    }

    // Regular line — parse inline markdown
    parse_inline_markdown_ir(line)
}

/// Parse inline markdown (bold, code, links) and return IR spans.
pub(super) fn parse_inline_markdown_ir(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut chars = text.chars().peekable();
    let mut current = String::new();

    while let Some(c) = chars.next() {
        match c {
            '`' => {
                // Inline code
                if !current.is_empty() {
                    spans.push(Span::new(std::mem::take(&mut current)));
                }

                let mut code = String::new();
                while let Some(&next) = chars.peek() {
                    if next == '`' {
                        let _r = chars.next();
                        break;
                    }
                    if let Some(ch) = chars.next() {
                        code.push(ch);
                    }
                }

                if !code.is_empty() {
                    spans.push(Span::styled(code, Semantic::Warning));
                }
            }
            '*' | '_' => {
                // Check for bold (**/__) — single markers are plain text
                let is_double = chars.peek() == Some(&c);

                if is_double {
                    let _r = chars.next(); // consume second marker

                    if !current.is_empty() {
                        spans.push(Span::new(std::mem::take(&mut current)));
                    }

                    // Bold text
                    let mut bold_text = String::new();
                    while let Some(next) = chars.next() {
                        if next == c && chars.peek() == Some(&c) {
                            let _r2 = chars.next();
                            break;
                        }
                        bold_text.push(next);
                    }

                    if !bold_text.is_empty() {
                        spans.push(Span::new(bold_text).bold());
                    }
                } else {
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
                    let _r = chars.next(); // consume (
                    for next in chars.by_ref() {
                        if next == ')' {
                            break;
                        }
                    }

                    // Display link text in accent color
                    if !current.is_empty() {
                        spans.push(Span::new(std::mem::take(&mut current)));
                    }
                    spans.push(Span::styled(link_text, Semantic::Accent));
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
        spans.push(Span::new(current));
    }

    if spans.is_empty() {
        spans.push(Span::new(String::new()));
    }

    spans
}
