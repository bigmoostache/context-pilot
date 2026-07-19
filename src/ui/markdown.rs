use cp_base::cast::Safe as _;
use cp_render::{Semantic, Span};
use unicode_width::UnicodeWidthChar as _;

/// Char iterator peekable over `char`, shared by the markdown width/parse walks.
type CharCursor<'cursor> = std::iter::Peekable<std::str::Chars<'cursor>>;

/// Consume an inline-code span (already past the opening backtick) and return
/// the display width of its content. Stops after the closing backtick.
fn width_code_span(chars: &mut CharCursor<'_>) -> usize {
    let mut width = 0usize;
    while let Some(&next) = chars.peek() {
        if next == '`' {
            let _r1 = chars.next();
            break;
        }
        width = width.saturating_add(chars.next().and_then(unicode_width::UnicodeWidthChar::width).unwrap_or(0));
    }
    width
}

/// Handle a `*`/`_` marker at `c`: bold span content width when doubled, else
/// the literal marker width.
fn width_emphasis(chars: &mut CharCursor<'_>, c: char) -> usize {
    if chars.peek() != Some(&c) {
        return c.width().unwrap_or(0);
    }
    let _r1 = chars.next(); // consume second marker
    let mut width = 0usize;
    while let Some(next) = chars.next() {
        if next == c && chars.peek() == Some(&c) {
            let _r2 = chars.next();
            break;
        }
        width = width.saturating_add(next.width().unwrap_or(0));
    }
    width
}

/// Handle a `[` at the cursor: link-text width for a valid `[text](url)`, else
/// the literal `[` + text (+ `]`) width.
fn width_link(chars: &mut CharCursor<'_>) -> usize {
    let mut link_text_len = 0usize;
    let mut found_bracket = false;
    for next in chars.by_ref() {
        if next == ']' {
            found_bracket = true;
            break;
        }
        link_text_len = link_text_len.saturating_add(next.width().unwrap_or(0));
    }
    if found_bracket && chars.peek() == Some(&'(') {
        let _r1 = chars.next(); // consume (
        for next in chars.by_ref() {
            if next == ')' {
                break;
            }
        }
        return link_text_len;
    }
    let mut width = '['.width().unwrap_or(0).saturating_add(link_text_len);
    if found_bracket {
        width = width.saturating_add(']'.width().unwrap_or(0));
    }
    width
}

/// Calculate the display width of text after stripping markdown markers.
fn markdown_display_width(text: &str) -> usize {
    let mut width = 0usize;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        width = width.saturating_add(match c {
            '`' => width_code_span(&mut chars),
            '*' | '_' => width_emphasis(&mut chars, c),
            '[' => width_link(&mut chars),
            _ => c.width().unwrap_or(0),
        });
    }

    width
}

/// Accumulator for [`wrap_cell_text`]: the emitted lines plus the in-progress
/// current line and its display width.
struct WrapState {
    /// Completed wrapped lines.
    lines: Vec<String>,
    /// Line currently being built.
    current: String,
    /// Display width of `current`.
    width: usize,
}

impl WrapState {
    /// Fresh state with no lines and an empty current line.
    const fn new() -> Self {
        Self { lines: Vec::new(), current: String::new(), width: 0 }
    }

    /// Break `word` (longer than `max`) char-by-char, flushing when it fills.
    fn push_broken_word(&mut self, word: &str, max: usize) {
        for ch in word.chars() {
            let cw = ch.width().unwrap_or(0);
            if self.width.saturating_add(cw) > max && self.width > 0 {
                self.lines.push(std::mem::take(&mut self.current));
                self.width = 0;
            }
            self.current.push(ch);
            self.width = self.width.saturating_add(cw);
        }
    }

    /// Start a fresh line with `word`, breaking it if it exceeds `max`.
    /// Assumes the current line is empty.
    fn start_line_with_word(&mut self, word: &str, word_width: usize, max: usize) {
        if word_width > max {
            self.push_broken_word(word, max);
        } else {
            self.current.push_str(word);
            self.width = word_width;
        }
    }
}

/// Wrap text to fit within a given width, breaking on word boundaries.
/// Returns a Vec of lines, each fitting within `width` characters.
fn wrap_cell_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_owned()];
    }
    if markdown_display_width(text) <= width {
        return vec![text.to_owned()];
    }

    let mut st = WrapState::new();

    for word in text.split_whitespace() {
        let word_width = markdown_display_width(word);

        if st.width == 0 {
            st.start_line_with_word(word, word_width, width);
        } else if st.width.saturating_add(1).saturating_add(word_width) <= width {
            // Fits on current line with a space
            st.current.push(' ');
            st.current.push_str(word);
            st.width = st.width.saturating_add(1).saturating_add(word_width);
        } else {
            // Doesn't fit — start a new line
            st.lines.push(std::mem::take(&mut st.current));
            st.start_line_with_word(word, word_width, width);
        }
    }

    if !st.current.is_empty() {
        st.lines.push(st.current);
    }

    if st.lines.is_empty() {
        st.lines.push(String::new());
    }

    st.lines
}

/// Parsed markdown table: cell rows + a per-row separator-row flag.
struct ParsedTable {
    /// Cell text per row.
    rows: Vec<Vec<String>>,
    /// `true` for separator rows (`|---|---|`), aligned with `rows`.
    is_separator_row: Vec<bool>,
}

/// Split raw table lines into trimmed cells, flagging separator rows.
fn parse_table_rows(table_lines: &[&str]) -> ParsedTable {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut is_separator_row: Vec<bool> = Vec::new();
    for line in table_lines {
        let trimmed = line.trim();
        let inner = trimmed.trim_start_matches('|').trim_end_matches('|');
        let cells: Vec<String> = inner.split('|').map(|c| c.trim().to_owned()).collect();
        let is_sep = cells.iter().all(|c| c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' '));
        is_separator_row.push(is_sep);
        rows.push(cells);
    }
    ParsedTable { rows, is_separator_row }
}

/// Max display width per column (separator rows excluded).
fn compute_col_widths(parsed: &ParsedTable, num_cols: usize) -> Vec<usize> {
    let mut col_widths: Vec<usize> = vec![0; num_cols];
    for (i, row) in parsed.rows.iter().enumerate() {
        if parsed.is_separator_row.get(i).copied().unwrap_or(false) {
            continue;
        }
        for (col, cell) in row.iter().enumerate() {
            if let Some(cw) = col_widths.get_mut(col) {
                *cw = (*cw).max(markdown_display_width(cell));
            }
        }
    }
    col_widths
}

/// Distribute `remaining` leftover columns of width to the widest columns first.
fn distribute_remaining(new_widths: &mut [usize], orig_widths: &[usize], num_cols: usize, mut remaining: usize) {
    let mut col_indices: Vec<usize> = (0..num_cols).collect();
    col_indices.sort_by(|&a, &b| {
        let width_a = orig_widths.get(a).copied().unwrap_or(0);
        let width_b = orig_widths.get(b).copied().unwrap_or(0);
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

/// Shrink columns proportionally to fit `max_width` when the natural table is
/// too wide. Returns the (possibly unchanged) column widths.
fn fit_col_widths(col_widths: Vec<usize>, num_cols: usize, max_width: usize) -> Vec<usize> {
    let separator_width = if num_cols > 1 { num_cols.saturating_sub(1).saturating_mul(3) } else { 0 };
    let total_content_width: usize = col_widths.iter().sum();
    let total_width = total_content_width.saturating_add(separator_width);

    if total_width <= max_width || max_width <= separator_width {
        return col_widths;
    }
    let available = max_width.saturating_sub(separator_width);
    let mut new_widths: Vec<usize> = col_widths
        .iter()
        .map(|&w| {
            let proportional = (w.to_f64() / total_content_width.to_f64() * available.to_f64()).to_usize();
            proportional.max(3)
        })
        .collect();
    let used: usize = new_widths.iter().sum();
    if used < available {
        distribute_remaining(&mut new_widths, &col_widths, num_cols, available.saturating_sub(used));
    }
    new_widths
}

/// Build one full-width border row (`left`…`mid`…`right` with `─` fills).
fn border_row(col_widths: &[usize], left: &str, mid: &str, right: &str) -> Vec<Span> {
    let mut spans: Vec<Span> = vec![Span::styled(left.to_owned(), Semantic::Border)];
    for (col, width) in col_widths.iter().enumerate() {
        if col > 0 {
            spans.push(Span::styled(mid.to_owned(), Semantic::Border));
        }
        spans.push(Span::styled("─".repeat(*width), Semantic::Border));
    }
    spans.push(Span::styled(right.to_owned(), Semantic::Border));
    spans
}

/// Render one display line of a data cell: content spans + right padding.
fn render_cell(spans: &mut Vec<Span>, cell_text: &str, width: usize, is_header: bool) {
    let display_width = markdown_display_width(cell_text);
    let padding = " ".repeat(width.saturating_sub(display_width));
    if is_header {
        spans.push(Span::styled(format!("{cell_text}{padding}"), Semantic::Accent).bold());
    } else if cell_text.is_empty() {
        spans.push(Span::new(" ".repeat(width)));
    } else {
        spans.extend(parse_inline_markdown(cell_text));
        if !padding.is_empty() {
            spans.push(Span::new(padding));
        }
    }
}

/// Render a data row (one logical row may wrap into several display lines).
fn render_data_row(row: &[String], col_widths: &[usize], is_header: bool, result: &mut Vec<Vec<Span>>) {
    let mut wrapped_cells: Vec<Vec<String>> = Vec::new();
    let mut max_lines = 1usize;
    for (col, width) in col_widths.iter().enumerate() {
        let cell = row.get(col).map_or("", |s| s.as_str());
        let cell_lines = wrap_cell_text(cell, *width);
        max_lines = max_lines.max(cell_lines.len());
        wrapped_cells.push(cell_lines);
    }

    for line_idx in 0..max_lines {
        let mut spans: Vec<Span> = vec![Span::styled("│ ".to_owned(), Semantic::Border)];
        for (col, width) in col_widths.iter().enumerate() {
            if col > 0 {
                spans.push(Span::styled(" │ ".to_owned(), Semantic::Border));
            }
            let cell_text = wrapped_cells.get(col).and_then(|lines| lines.get(line_idx)).map_or("", |s| s.as_str());
            render_cell(&mut spans, cell_text, *width, is_header);
        }
        spans.push(Span::styled(" │".to_owned(), Semantic::Border));
        result.push(spans);
    }
}

/// Render a markdown table with aligned columns.
///
/// Strategy: compute fixed column widths -> for each row, wrap cell text to fit
/// -> render each display line as a sequence of fixed-width cells separated by |.
/// Vertical separators are always at the same character positions.
pub(crate) fn render_markdown_table(table_lines: &[&str], max_width: usize) -> Vec<Vec<Span>> {
    let parsed = parse_table_rows(table_lines);
    let num_cols = parsed.rows.iter().map(Vec::len).max().unwrap_or(0);
    let col_widths = fit_col_widths(compute_col_widths(&parsed, num_cols), num_cols, max_width);

    let mut result: Vec<Vec<Span>> = Vec::new();
    result.push(border_row(&col_widths, "┌─", "─┬─", "─┐")); // top border

    for (row_idx, row) in parsed.rows.iter().enumerate() {
        if parsed.is_separator_row.get(row_idx).copied().unwrap_or(false) {
            result.push(border_row(&col_widths, "├─", "─┼─", "─┤"));
            continue;
        }
        render_data_row(row, &col_widths, row_idx == 0, &mut result);

        // Thin separator between consecutive data rows.
        let next_row_idx = row_idx.saturating_add(1);
        if next_row_idx < parsed.rows.len() && !parsed.is_separator_row.get(next_row_idx).copied().unwrap_or(false) {
            result.push(border_row(&col_widths, "├─", "─┼─", "─┤"));
        }
    }

    result.push(border_row(&col_widths, "└─", "─┴─", "─┘")); // bottom border
    result
}

/// Flush the pending plain-text buffer into `spans` (no-op when empty).
fn flush_plain(spans: &mut Vec<Span>, current: &mut String) {
    if !current.is_empty() {
        spans.push(Span::new(std::mem::take(current)));
    }
}

/// Emit an inline-code span (cursor already past the opening backtick).
fn parse_code_span(spans: &mut Vec<Span>, current: &mut String, chars: &mut CharCursor<'_>) {
    flush_plain(spans, current);
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
        spans.push(Span::styled(code, Semantic::Warning));
    }
}

/// Handle a `*`/`_`: emit a bold span when doubled, else buffer the literal.
fn parse_emphasis(spans: &mut Vec<Span>, current: &mut String, chars: &mut CharCursor<'_>, c: char) {
    if chars.peek() != Some(&c) {
        current.push(c); // single marker — literal
        return;
    }
    let _r1 = chars.next(); // consume second */_
    flush_plain(spans, current);
    let mut bold_text = String::new();
    while let Some(next) = chars.next() {
        if next == c && chars.peek() == Some(&c) {
            let _r2 = chars.next(); // consume closing **
            break;
        }
        bold_text.push(next);
    }
    if !bold_text.is_empty() {
        spans.push(Span::new(bold_text).bold());
    }
}

/// Handle a `[`: emit an accent link-text span for a valid `[text](url)`, else
/// buffer the literal `[`+text(+`]`) back into `current`.
fn parse_link(spans: &mut Vec<Span>, current: &mut String, chars: &mut CharCursor<'_>) {
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
        for next in chars.by_ref() {
            if next == ')' {
                break;
            }
        }
        flush_plain(spans, current);
        spans.push(Span::styled(link_text, Semantic::Accent));
    } else {
        current.push('[');
        current.push_str(&link_text);
        if found_bracket {
            current.push(']');
        }
    }
}

/// Parse inline markdown (bold, italic, code) and return IR spans.
pub(crate) fn parse_inline_markdown(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut chars = text.chars().peekable();
    let mut current = String::new();

    while let Some(c) = chars.next() {
        match c {
            '`' => parse_code_span(&mut spans, &mut current, &mut chars),
            '*' | '_' => parse_emphasis(&mut spans, &mut current, &mut chars, c),
            '[' => parse_link(&mut spans, &mut current, &mut chars),
            _ => current.push(c),
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
