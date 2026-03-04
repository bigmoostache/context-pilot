//! Shared UI helpers for panel rendering.
//!
//! Provides Cell, Align, and render_table so that extracted module crates
//! can render tables without depending on the main binary.

use ratatui::prelude::*;
use unicode_width::UnicodeWidthStr;

use crate::config::theme;

/// Column alignment for table cells
#[derive(Debug, Clone, Copy, Default)]
pub enum Align {
    #[default]
    Left,
    Right,
}

/// A single table cell with text, style, and alignment
#[derive(Debug)]
pub struct Cell {
    pub text: String,
    pub style: Style,
    pub align: Align,
}

impl Cell {
    pub fn new(text: impl Into<String>, style: Style) -> Self {
        Self { text: text.into(), style, align: Align::Left }
    }
    pub fn right(text: impl Into<String>, style: Style) -> Self {
        Self { text: text.into(), style, align: Align::Right }
    }
}

/// Pad a string to a target display width using spaces, respecting alignment.
fn pad_to_width(text: &str, target: usize, align: Align) -> String {
    let w = UnicodeWidthStr::width(text);
    let deficit = target.saturating_sub(w);
    match align {
        Align::Left => format!("{}{}", text, " ".repeat(deficit)),
        Align::Right => format!("{}{}", " ".repeat(deficit), text),
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
pub fn render_table<'a>(header: &[Cell], rows: &[Vec<Cell>], footer: Option<&[Cell]>, indent: usize) -> Vec<Line<'a>> {
    let num_cols = header.len();

    // Compute column widths from header + all rows + footer using display width
    let mut col_widths: Vec<usize> = header.iter().map(|c| UnicodeWidthStr::width(c.text.as_str())).collect();
    col_widths.resize(num_cols, 0);

    for row in rows {
        for (col, cell) in row.iter().enumerate() {
            if col < num_cols {
                col_widths[col] = col_widths[col].max(UnicodeWidthStr::width(cell.text.as_str()));
            }
        }
    }
    if let Some(f) = footer {
        for (col, cell) in f.iter().enumerate() {
            if col < num_cols {
                col_widths[col] = col_widths[col].max(UnicodeWidthStr::width(cell.text.as_str()));
            }
        }
    }

    let pad = " ".repeat(indent);
    let mut lines: Vec<Line<'_>> = Vec::new();

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
                let padded = pad_to_width(&cell.text, *col_w, cell.align);
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
            let padded = pad_to_width(&hdr.text, w, hdr.align);
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

/// Simple text-cell for `render_table_text`. Style-free, just text + alignment.
#[derive(Debug)]
pub struct TextCell {
    pub text: String,
    pub align: Align,
}

impl TextCell {
    pub fn left(text: impl Into<String>) -> Self {
        Self { text: text.into(), align: Align::Left }
    }
    pub fn right(text: impl Into<String>) -> Self {
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
pub fn render_table_text(header: &[&str], rows: &[Vec<TextCell>]) -> String {
    let num_cols = header.len();

    // Compute column widths using display width
    let mut col_widths: Vec<usize> = header.iter().map(|h| UnicodeWidthStr::width(*h)).collect();
    col_widths.resize(num_cols, 0);

    for row in rows {
        for (col, cell) in row.iter().enumerate() {
            if col < num_cols {
                col_widths[col] = col_widths[col].max(UnicodeWidthStr::width(cell.text.as_str()));
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
        output.push_str(&pad(hdr, col_widths[col], Align::Left));
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
    if let Some(start) = line.find(" (") {
        let rest = &line[start + 2..];
        if let Some(end_paren) = rest.find(')') {
            let inner = &rest[..end_paren];
            if inner.ends_with(" child") || inner.ends_with(" children") {
                let num_part = inner.split_whitespace().next()?;
                if num_part.parse::<usize>().is_ok() {
                    return Some((start + 1, start + 2 + end_paren + 1));
                }
            }
        }
    }
    None
}

// =============================================================================
// Question Form Types (AskUserQuestion tool #39)
// =============================================================================

/// A single option the user can choose.
#[derive(Debug, Clone)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

/// A single question with its options.
#[derive(Debug, Clone)]
pub struct Question {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,
}

/// Per-question answer state tracked during form interaction.
#[derive(Debug, Clone)]
pub struct QuestionAnswer {
    /// Index of the currently highlighted option (0-based, includes "Other" at end)
    pub cursor: usize,
    /// Which option indices are selected (for single-select: at most one)
    pub selected: Vec<usize>,
    /// If "Other" is selected, the user's typed text
    pub other_text: String,
    /// Whether the user is currently typing in the "Other" field
    pub typing_other: bool,
}

impl Default for QuestionAnswer {
    fn default() -> Self {
        Self::new()
    }
}

impl QuestionAnswer {
    pub fn new() -> Self {
        Self { cursor: 0, selected: Vec::new(), other_text: String::new(), typing_other: false }
    }
}

/// The full pending question form state, stored in State.module_data via ext.
#[derive(Debug, Clone)]
pub struct PendingQuestionForm {
    /// The tool_use_id this form was created for (needed to produce ToolResult)
    pub tool_use_id: String,
    /// The questions to present
    pub questions: Vec<Question>,
    /// Current question index (0-based)
    pub current_question: usize,
    /// Per-question answer state
    pub answers: Vec<QuestionAnswer>,
    /// Whether the form has been resolved (submitted or dismissed)
    pub resolved: bool,
    /// The final JSON result string (set on submit/dismiss)
    pub result_json: Option<String>,
}

impl PendingQuestionForm {
    pub fn new(tool_use_id: String, questions: Vec<Question>) -> Self {
        let answers = questions.iter().map(|_| QuestionAnswer::new()).collect();
        Self { tool_use_id, questions, current_question: 0, answers, resolved: false, result_json: None }
    }

    /// Total number of options for the current question (including "Other")
    pub fn current_option_count(&self) -> usize {
        self.questions[self.current_question].options.len() + 1 // +1 for "Other"
    }

    /// Index of the "Other" option for the current question
    pub fn other_index(&self) -> usize {
        self.questions[self.current_question].options.len()
    }

    /// Whether current question is multi-select
    pub fn is_multi_select(&self) -> bool {
        self.questions[self.current_question].multi_select
    }

    /// Move cursor up
    pub fn cursor_up(&mut self) {
        let other_idx = self.questions[self.current_question].options.len();
        let ans = &mut self.answers[self.current_question];
        if ans.cursor > 0 {
            ans.cursor -= 1;
        }
        ans.typing_other = ans.cursor == other_idx;
    }

    /// Move cursor down
    pub fn cursor_down(&mut self) {
        let option_count = self.questions[self.current_question].options.len() + 1;
        let other_idx = self.questions[self.current_question].options.len();
        let ans = &mut self.answers[self.current_question];
        let max = option_count - 1;
        if ans.cursor < max {
            ans.cursor += 1;
        }
        ans.typing_other = ans.cursor == other_idx;
    }

    /// Toggle selection on current cursor position (for multi-select or single-select)
    pub fn toggle_selection(&mut self) {
        let q_idx = self.current_question;
        let ans = &mut self.answers[q_idx];
        let cursor = ans.cursor;
        let other_idx = self.questions[q_idx].options.len();

        if cursor == other_idx {
            // "Other" selected — start typing mode
            ans.typing_other = true;
            // Clear other selections if single-select
            if !self.questions[q_idx].multi_select {
                ans.selected.clear();
            }
            return;
        }

        if self.questions[q_idx].multi_select {
            // Toggle in selected list
            if let Some(pos) = ans.selected.iter().position(|&s| s == cursor) {
                ans.selected.remove(pos);
            } else {
                ans.selected.push(cursor);
            }
            ans.typing_other = false;
        } else {
            // Single select — replace
            ans.selected = vec![cursor];
            ans.typing_other = false;
            ans.other_text.clear();
        }
    }

    /// Handle Enter: for single-select, select current + advance. For multi-select, advance.
    pub fn handle_enter(&mut self) {
        let q_idx = self.current_question;
        let ans = &self.answers[q_idx];

        // For single-select: if nothing selected and not typing other, select current cursor
        if !self.questions[q_idx].multi_select && ans.selected.is_empty() && !ans.typing_other {
            self.toggle_selection();
        }

        // Advance to next question or resolve
        if self.current_question < self.questions.len() - 1 {
            self.current_question += 1;
        } else {
            self.submit();
        }
    }

    /// Dismiss the form (Esc)
    pub fn dismiss(&mut self) {
        self.resolved = true;
        self.result_json = Some(r#"{"dismissed":true,"message":"User declined to answer"}"#.to_string());
    }

    /// Submit all answers
    pub fn submit(&mut self) {
        self.resolved = true;

        let mut answers_json = Vec::new();
        for (i, q) in self.questions.iter().enumerate() {
            let ans = &self.answers[i];

            let selected: Vec<String> =
                ans.selected.iter().filter_map(|&idx| q.options.get(idx).map(|o| o.label.clone())).collect();

            let other = if ans.typing_other && !ans.other_text.is_empty() {
                format!(r#""{}""#, ans.other_text.replace('"', "\\\""))
            } else {
                "null".to_string()
            };

            answers_json.push(format!(
                r#"{{"header":"{}","selected":[{}],"other_text":{}}}"#,
                q.header.replace('"', "\\\""),
                selected.iter().map(|s| format!(r#""{}""#, s.replace('"', "\\\""))).collect::<Vec<_>>().join(","),
                other
            ));
        }

        self.result_json = Some(format!(r#"{{"answers":[{}]}}"#, answers_json.join(",")));
    }

    /// Type a character into the "Other" text field
    pub fn type_char(&mut self, c: char) {
        let ans = &mut self.answers[self.current_question];
        if ans.typing_other {
            ans.other_text.push(c);
        }
    }

    /// Backspace in the "Other" text field
    pub fn backspace(&mut self) {
        let ans = &mut self.answers[self.current_question];
        if ans.typing_other {
            ans.other_text.pop();
        }
    }

    /// Go to previous question (Left arrow). Always allowed if not on first.
    pub fn prev_question(&mut self) {
        if self.current_question > 0 {
            self.current_question -= 1;
        }
    }

    /// Go to next question (Right arrow). Only allowed if current question has an answer.
    pub fn next_question(&mut self) {
        if self.current_question < self.questions.len() - 1 && self.current_question_answered() {
            self.current_question += 1;
        }
    }

    /// Check if the current question has been answered (selection or other text)
    pub fn current_question_answered(&self) -> bool {
        let ans = &self.answers[self.current_question];
        !ans.selected.is_empty() || (ans.typing_other && !ans.other_text.is_empty())
    }
}
