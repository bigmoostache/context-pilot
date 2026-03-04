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
    pub question: String,
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
    pub fn new() -> Self {
        Self { cursor: 0, selected: Vec::new(), other_text: String::new(), typing_other: false }
    }
}

/// The full pending question form state, stored in `State.module_data` via ext.
#[derive(Debug, Clone)]
pub struct PendingQuestionForm {
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

impl PendingQuestionForm {
    /// Create a new question form from a tool call and its questions.
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
            _ = ans.other_text.pop();
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
