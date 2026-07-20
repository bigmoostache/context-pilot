//! Markdown-to-IR parser: converts markdown text into `Vec<Span>` or `Vec<Block>`.
//!
//! Handles inline formatting (bold, code, links), block-level structures
//! (headers, bullets, fenced code blocks), and produces platform-agnostic
//! IR types consumable by any adapter.

use crate::{Align, Block, Cell, Column, Semantic, Span};

/// Bullet-point prefix glyph: U+2022 "• " (escaped to keep the source ASCII-only).
const BULLET_PREFIX: &str = "\u{2022} ";

/// Index one past the last consecutive `|`-prefixed line starting at `start`.
///
/// A markdown table is a run of pipe-prefixed lines; this finds its end so
/// [`to_blocks`] can slice it out without an inline accumulation loop.
fn table_end(lines: &[&str], start: usize) -> usize {
    let mut i = start.saturating_add(1);
    while i < lines.len() {
        if lines.get(i).copied().unwrap_or("").trim_start().starts_with('|') {
            i = i.saturating_add(1);
        } else {
            break;
        }
    }
    i
}

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

        // Markdown table: slice out the run of consecutive `|`-prefixed lines.
        if line.trim_start().starts_with('|') {
            let end = table_end(&lines, i);
            blocks.push(parse_table(lines.get(i..end).unwrap_or(&[])));
            i = end;
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
        let mut spans =
            vec![Span::new(" ".repeat(indent)), Span::styled(BULLET_PREFIX.to_owned(), Semantic::AccentDim)];
        spans.extend(parse_inline(stripped));
        return spans;
    }

    if trimmed.starts_with("* ") && !trimmed.starts_with("**") {
        let content = trimmed.get(2..).unwrap_or("");
        let indent = line.len().saturating_sub(trimmed.len());
        let mut spans =
            vec![Span::new(" ".repeat(indent)), Span::styled(BULLET_PREFIX.to_owned(), Semantic::AccentDim)];
        spans.extend(parse_inline(content));
        return spans;
    }

    // Regular line — parse inline markdown
    parse_inline(line)
}

/// Flush any pending plain-text buffer into `spans` as a single span.
fn flush_text(spans: &mut Vec<Span>, current: &mut String) {
    if !current.is_empty() {
        spans.push(Span::new(std::mem::take(current)));
    }
}

/// Read an inline-code body: characters up to (and consuming) the closing `` ` ``.
fn take_code(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
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
    code
}

/// Read a bold body after a `**`/`__` open marker (`marker` is the marker char),
/// up to (and consuming) the matching double marker.
fn take_bold(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, marker: char) -> String {
    let mut text = String::new();
    while let Some(next) = chars.next() {
        if next == marker && chars.peek() == Some(&marker) {
            let _r = chars.next();
            break;
        }
        text.push(next);
    }
    text
}

/// Outcome of parsing a `[...]` sequence: either a valid link's display text,
/// or the literal characters to keep when it is not a well-formed link.
enum LinkParse {
    /// `[text](url)` — carries the display text (url discarded).
    Link(String),
    /// Malformed — carries the literal text to append verbatim.
    Literal(String),
}

/// Parse `[text](url)` after the opening `[` has been consumed.
fn take_link(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> LinkParse {
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
        LinkParse::Link(link_text)
    } else {
        let mut literal = String::from('[');
        literal.push_str(&link_text);
        if found_bracket {
            literal.push(']');
        }
        LinkParse::Literal(literal)
    }
}

/// Handle a `` ` `` inline-code marker: flush pending text, then push the code span.
fn handle_code(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, spans: &mut Vec<Span>, current: &mut String) {
    flush_text(spans, current);
    let code = take_code(chars);
    if !code.is_empty() {
        spans.push(Span::styled(code, Semantic::Warning));
    }
}

/// Handle a `*`/`_` marker: a doubled marker opens bold, a lone one is literal.
fn handle_marker(
    marker: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    spans: &mut Vec<Span>,
    current: &mut String,
) {
    if chars.peek() != Some(&marker) {
        current.push(marker);
        return;
    }
    let _r = chars.next(); // consume the second marker
    flush_text(spans, current);
    let bold = take_bold(chars, marker);
    if !bold.is_empty() {
        spans.push(Span::new(bold).bold());
    }
}

/// Handle a `[` link opener: push an accent span for a valid `[text](url)`,
/// otherwise append the literal characters verbatim.
fn handle_link(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, spans: &mut Vec<Span>, current: &mut String) {
    match take_link(chars) {
        LinkParse::Link(link_text) => {
            flush_text(spans, current);
            spans.push(Span::styled(link_text, Semantic::Accent));
        }
        LinkParse::Literal(literal) => current.push_str(&literal),
    }
}

/// Parse inline markdown (bold, code, links) and return IR spans.
#[must_use]
pub fn parse_inline(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut chars = text.chars().peekable();
    let mut current = String::new();

    while let Some(c) = chars.next() {
        match c {
            '`' => handle_code(&mut chars, &mut spans, &mut current),
            '*' | '_' => handle_marker(c, &mut chars, &mut spans, &mut current),
            '[' => handle_link(&mut chars, &mut spans, &mut current),
            _ => current.push(c),
        }
    }

    flush_text(&mut spans, &mut current);

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
