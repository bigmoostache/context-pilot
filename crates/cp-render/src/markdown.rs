//! Markdown-to-IR parser: converts markdown text into `Vec<Span>` or `Vec<Block>`.
//!
//! Handles inline formatting (bold, code, links), block-level structures
//! (headers, bullets, fenced code blocks), and produces platform-agnostic
//! IR types consumable by any adapter.

use crate::{Align, Block, Cell, Column, Semantic, Span};

/// Parse a full markdown document into IR blocks.
///
/// Handles fenced code blocks (`` ``` ``), headers, bullets, and inline
/// formatting. Each output block is one visual line.
#[must_use]
pub fn to_blocks(content: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    let mut in_code_block = false;

    while i < lines.len() {
        let Some(&line) = lines.get(i) else { break };

        // Detect fenced code block boundaries (``` with optional language tag)
        if line.trim().starts_with("```") {
            in_code_block = !in_code_block;
            blocks.push(Block::Line(vec![Span::styled(line.to_owned(), Semantic::Muted)]));
            i = i.saturating_add(1);
            continue;
        }

        // Inside a code block: render verbatim — no wrapping, no markdown parsing.
        if in_code_block {
            blocks.push(Block::Line(vec![Span::styled(line.to_owned(), Semantic::Code)]));
            i = i.saturating_add(1);
            continue;
        }

        if line.is_empty() {
            blocks.push(Block::Empty);
            i = i.saturating_add(1);
            continue;
        }

        // Markdown table: accumulate consecutive lines starting with |
        if line.trim_start().starts_with('|') {
            let mut table_lines = vec![line];
            i = i.saturating_add(1);
            while i < lines.len() {
                let next = lines.get(i).copied().unwrap_or("");
                if next.trim_start().starts_with('|') {
                    table_lines.push(next);
                    i = i.saturating_add(1);
                } else {
                    break;
                }
            }
            blocks.push(parse_table(&table_lines));
            continue;
        }

        // Regular line — parse markdown
        blocks.push(Block::Line(parse_line(line)));
        i = i.saturating_add(1);
    }

    blocks
}

/// Parse a single markdown line and return IR spans.
///
/// Handles headers (`#`), bullet points (`- `, `* `), and inline formatting.
#[must_use]
pub fn parse_line(line: &str) -> Vec<Span> {
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
        spans.extend(parse_inline(stripped));
        return spans;
    }

    if trimmed.starts_with("* ") && !trimmed.starts_with("**") {
        let content = trimmed.get(2..).unwrap_or("");
        let indent = line.len().saturating_sub(trimmed.len());
        let mut spans = vec![Span::new(" ".repeat(indent)), Span::styled("• ".to_owned(), Semantic::AccentDim)];
        spans.extend(parse_inline(content));
        return spans;
    }

    // Regular line — parse inline markdown
    parse_inline(line)
}

/// Parse inline markdown (bold, code, links) and return IR spans.
#[must_use]
pub fn parse_inline(text: &str) -> Vec<Span> {
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

// ── Table parsing helpers ────────────────────────────────────────────

/// Split a markdown table row by `|` and trim each cell.
///
/// `| a | b | c |` → `["a", "b", "c"]`
fn split_table_row(line: &str) -> Vec<&str> {
    let trimmed = line.trim();
    // Strip leading/trailing pipes, then split by |
    let inner = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    inner.split('|').map(str::trim).collect()
}

/// Check if a row is a separator (all cells match `:?-+:?`).
fn is_separator_row(cells: &[&str]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|c| {
            let s = c.trim();
            if s.is_empty() {
                return false;
            }
            let s = s.strip_prefix(':').unwrap_or(s);
            let s = s.strip_suffix(':').unwrap_or(s);
            !s.is_empty() && s.chars().all(|ch| ch == '-')
        })
}

/// Parse alignment from a separator cell (`:---:` → Center, `---:` → Right, else Left).
fn parse_alignment(cell: &str) -> Align {
    let s = cell.trim();
    let left = s.starts_with(':');
    let right = s.ends_with(':');
    match (left, right) {
        (true, true) => Align::Center,
        (false, true) => Align::Right,
        _ => Align::Left,
    }
}

/// Parse accumulated markdown table lines into a `Block::Table`.
fn parse_table(lines: &[&str]) -> Block {
    if lines.is_empty() {
        return Block::Empty;
    }

    // Parse header row
    let header_cells = split_table_row(lines.first().copied().unwrap_or(""));

    // Check for separator row (line index 1)
    let (alignments, data_start) = if let Some(&sep_line) = lines.get(1) {
        let sep_cells = split_table_row(sep_line);
        if is_separator_row(&sep_cells) {
            let aligns: Vec<Align> = sep_cells.iter().map(|c| parse_alignment(c)).collect();
            (aligns, 2)
        } else {
            (vec![Align::Left; header_cells.len()], 1)
        }
    } else {
        (vec![Align::Left; header_cells.len()], 1)
    };

    // Build columns
    let columns: Vec<Column> = header_cells
        .iter()
        .enumerate()
        .map(|(idx, &h)| Column { header: h.to_owned(), align: alignments.get(idx).copied().unwrap_or(Align::Left) })
        .collect();

    // Build data rows
    let col_count = columns.len();
    let rows: Vec<Vec<Cell>> = lines
        .get(data_start..)
        .unwrap_or(&[])
        .iter()
        .map(|line| {
            let cells = split_table_row(line);
            let mut row: Vec<Cell> = cells
                .iter()
                .enumerate()
                .take(col_count)
                .map(|(idx, &text)| {
                    let align = alignments.get(idx).copied().unwrap_or(Align::Left);
                    Cell { spans: parse_inline(text), align }
                })
                .collect();
            // Pad with empty cells if row is short
            while row.len() < col_count {
                row.push(Cell::empty());
            }
            row
        })
        .collect();

    Block::Table { columns, rows }
}
