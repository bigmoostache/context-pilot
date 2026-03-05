//! Interactive question form types for the `ask_user_question` tool.
//!
//! Manages multi-question forms with single/multi-select options,
//! cursor navigation, and an auto-appended "Other" free-text option.

/// A single option the user can choose.
#[derive(Debug, Clone)]
pub struct QuestionOption {
    /// Short label text (1-5 words).
    pub label: String,
    /// Explanation of what this option means.
    pub description: String,
}

/// A single question with its options.
#[derive(Debug, Clone)]
pub struct Question {
    /// The complete question text.
    pub text: String,
    /// Very short label (max 12 chars) for compact display.
    pub header: String,
    /// Available choices (an "Other" free-text option is appended automatically).
    pub options: Vec<QuestionOption>,
    /// Whether the user can select multiple options.
    pub multi_select: bool,
}

/// Per-question answer state tracked during form interaction.
#[derive(Debug, Clone)]
pub struct QuestionAnswer {
    /// Index of the currently highlighted option (0-based, includes "Other" at end).
    pub cursor: usize,
    /// Which option indices are selected (for single-select: at most one).
    pub selected: Vec<usize>,
    /// If "Other" is selected, the user's typed text.
    pub other_text: String,
    /// Whether the user is currently typing in the "Other" field.
    pub typing_other: bool,
}

impl Default for QuestionAnswer {
    fn default() -> Self {
        Self::new()
    }
}

impl QuestionAnswer {
    /// Create a blank answer state (no selection, cursor at top).
    #[must_use]
    pub const fn new() -> Self {
        Self { cursor: 0, selected: Vec::new(), other_text: String::new(), typing_other: false }
    }
}

/// The full pending question form state, stored in `State.module_data` via ext.
#[derive(Debug, Clone)]
pub struct PendingForm {
    /// The `tool_use_id` this form was created for (needed to produce `ToolResult`)
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

impl PendingForm {
    /// Create a new question form from a tool call and its questions.
    #[must_use]
    pub fn new(tool_use_id: String, questions: Vec<Question>) -> Self {
        let answers = questions.iter().map(|_| QuestionAnswer::new()).collect();
        Self { tool_use_id, questions, current_question: 0, answers, resolved: false, result_json: None }
    }

    /// Total number of options for the current question (including "Other")
    #[must_use]
    pub fn current_option_count(&self) -> usize {
        let Some(q) = self.questions.get(self.current_question) else { return 1 };
        q.options.len().saturating_add(1) // +1 for "Other"
    }

    /// Index of the "Other" option for the current question
    #[must_use]
    pub fn other_index(&self) -> usize {
        let Some(q) = self.questions.get(self.current_question) else { return 0 };
        q.options.len()
    }

    /// Whether current question is multi-select
    #[must_use]
    pub fn is_multi_select(&self) -> bool {
        let Some(q) = self.questions.get(self.current_question) else { return false };
        q.multi_select
    }

    /// Move cursor up
    pub fn cursor_up(&mut self) {
        let Some(q) = self.questions.get(self.current_question) else { return };
        let other_idx = q.options.len();
        let Some(ans) = self.answers.get_mut(self.current_question) else { return };
        if ans.cursor > 0 {
            ans.cursor = ans.cursor.saturating_sub(1);
        }
        ans.typing_other = ans.cursor == other_idx;
    }

    /// Move cursor down
    pub fn cursor_down(&mut self) {
        let Some(q) = self.questions.get(self.current_question) else { return };
        let option_count = q.options.len().saturating_add(1);
        let other_idx = q.options.len();
        let Some(ans) = self.answers.get_mut(self.current_question) else { return };
        let max = option_count.saturating_sub(1);
        if ans.cursor < max {
            ans.cursor = ans.cursor.saturating_add(1);
        }
        ans.typing_other = ans.cursor == other_idx;
    }

    /// Toggle selection on current cursor position (for multi-select or single-select)
    pub fn toggle_selection(&mut self) {
        let q_idx = self.current_question;
        let cursor = {
            let Some(ans) = self.answers.get(q_idx) else { return };
            ans.cursor
        };
        let Some(q) = self.questions.get(q_idx) else { return };
        let other_idx = q.options.len();
        let multi_select = q.multi_select;

        if cursor == other_idx {
            // "Other" selected — start typing mode
            let Some(other_ans) = self.answers.get_mut(q_idx) else { return };
            other_ans.typing_other = true;
            // Clear other selections if single-select
            if !multi_select {
                other_ans.selected.clear();
            }
            return;
        }

        let Some(ans) = self.answers.get_mut(q_idx) else { return };
        if multi_select {
            // Toggle in selected list
            if let Some(pos) = ans.selected.iter().position(|&s| s == cursor) {
                _ = ans.selected.remove(pos);
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
        let Some(q) = self.questions.get(q_idx) else { return };
        let multi_select = q.multi_select;
        let Some(ans) = self.answers.get(q_idx) else { return };
        let selected_empty = ans.selected.is_empty();
        let typing_other = ans.typing_other;

        // For single-select: if nothing selected and not typing other, select current cursor
        if !multi_select && selected_empty && !typing_other {
            self.toggle_selection();
        }

        // Advance to next question or resolve
        if self.current_question < self.questions.len().saturating_sub(1) {
            self.current_question = self.current_question.saturating_add(1);
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
            let Some(ans) = self.answers.get(i) else { continue };

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
        let Some(ans) = self.answers.get_mut(self.current_question) else { return };
        if ans.typing_other {
            ans.other_text.push(c);
        }
    }

    /// Backspace in the "Other" text field
    pub fn backspace(&mut self) {
        let Some(ans) = self.answers.get_mut(self.current_question) else { return };
        if ans.typing_other {
            _ = ans.other_text.pop();
        }
    }

    /// Go to previous question (Left arrow). Always allowed if not on first.
    pub const fn prev_question(&mut self) {
        if self.current_question > 0 {
            self.current_question = self.current_question.saturating_sub(1);
        }
    }

    /// Go to next question (Right arrow). Only allowed if current question has an answer.
    pub fn next_question(&mut self) {
        if self.current_question < self.questions.len().saturating_sub(1) && self.current_question_answered() {
            self.current_question = self.current_question.saturating_add(1);
        }
    }

    /// Check if the current question has been answered (selection or other text)
    #[must_use]
    pub fn current_question_answered(&self) -> bool {
        let Some(ans) = self.answers.get(self.current_question) else { return false };
        !ans.selected.is_empty() || (ans.typing_other && !ans.other_text.is_empty())
    }
}
