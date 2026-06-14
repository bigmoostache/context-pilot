//! Thread-embedded interactive question forms.
//!
//! Questions are stored as raw JSON on a [`ThreadMessage`](crate::types::ThreadMessage)
//! and parsed into a [`ThreadQuestionForm`] for interactive rendering in the
//! Threads view. The form tracks per-question answer state (selection, "Other"
//! free-text) and serializes answers back to YAML for the reply message.

use serde::{Deserialize, Serialize};

/// A single option in a thread question.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadQuestionOption {
    /// Short label text (1-5 words).
    pub label: String,
    /// Explanation of what this option means.
    pub description: String,
}

/// A single question with its options and current answer state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadQuestion {
    /// Very short label (max 12 chars).
    pub header: String,
    /// The complete question text.
    pub text: String,
    /// Available choices (an "Other" free-text option is appended at render time).
    pub options: Vec<ThreadQuestionOption>,
    /// Whether the user can select multiple options.
    pub multi_select: bool,
    /// Index of the currently highlighted option (0-based, includes "Other" at end).
    #[serde(default)]
    pub cursor: usize,
    /// Which option indices are selected.
    #[serde(default)]
    pub selected: Vec<usize>,
    /// Whether the user is currently typing in the "Other" field.
    #[serde(default)]
    pub typing_other: bool,
    /// The user's typed text for "Other".
    #[serde(default)]
    pub other_text: String,
}

/// Active question form state for thread-embedded questions.
///
/// Tracks which thread has pending questions, the parsed questions
/// with per-question answer state, and the currently focused question.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadQuestionForm {
    /// Thread ID this question form belongs to.
    pub thread_id: String,
    /// Parsed questions with answer state.
    pub questions: Vec<ThreadQuestion>,
    /// Index of the currently focused question (for multi-question forms).
    pub focused_index: usize,
}

/// Truncate a string to at most `max` characters, appending "…" if truncated.
fn cap_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut r: String = s.chars().take(max).collect();
    r.push('…');
    r
}

impl ThreadQuestionForm {
    /// Parse a question form from a thread message's JSON question field.
    ///
    /// Returns `None` if the JSON is malformed or empty.
    /// Caps: 50 questions × 100 options, header 50 chars, text/desc 2000, label 200.
    #[must_use]
    pub fn from_json(thread_id: &str, json: &serde_json::Value) -> Option<Self> {
        let arr = json.as_array()?;
        if arr.is_empty() {
            return None;
        }

        let questions: Vec<ThreadQuestion> = arr
            .iter()
            .take(50)
            .filter_map(|q| {
                let header = cap_str(q.get("header")?.as_str()?, 50);
                let text = cap_str(q.get("question")?.as_str()?, 2_000);
                let multi_select = q.get("multiSelect").and_then(serde_json::Value::as_bool).unwrap_or(false);
                let options = q
                    .get("options")?
                    .as_array()?
                    .iter()
                    .take(100)
                    .filter_map(|o| {
                        Some(ThreadQuestionOption {
                            label: cap_str(o.get("label")?.as_str()?, 200),
                            description: cap_str(o.get("description")?.as_str()?, 2_000),
                        })
                    })
                    .collect();
                Some(ThreadQuestion {
                    header,
                    text,
                    options,
                    multi_select,
                    cursor: 0,
                    selected: Vec::new(),
                    typing_other: false,
                    other_text: String::new(),
                })
            })
            .collect();
        if questions.is_empty() {
            return None;
        }
        Some(Self { thread_id: thread_id.to_owned(), questions, focused_index: 0 })
    }

    /// Total number of options for the current question (including "Other").
    #[must_use]
    pub fn current_option_count(&self) -> usize {
        self.questions.get(self.focused_index).map_or(1, |q| q.options.len().saturating_add(1))
    }

    /// Index of the "Other" option for the current question.
    #[must_use]
    pub fn other_index(&self) -> usize {
        self.questions.get(self.focused_index).map_or(0, |q| q.options.len())
    }

    /// Move cursor up within the current question.
    pub fn cursor_up(&mut self) {
        let Some(q) = self.questions.get_mut(self.focused_index) else { return };
        let other_idx = q.options.len();
        if q.cursor > 0 {
            q.cursor = q.cursor.saturating_sub(1);
        }
        q.typing_other = q.cursor == other_idx;
    }

    /// Move cursor down within the current question.
    pub fn cursor_down(&mut self) {
        let Some(q) = self.questions.get_mut(self.focused_index) else { return };
        let max = q.options.len(); // "Other" is at this index
        if q.cursor < max {
            q.cursor = q.cursor.saturating_add(1);
        }
        q.typing_other = q.cursor == q.options.len();
    }

    /// Toggle selection on current cursor position.
    pub fn toggle_selection(&mut self) {
        let Some(q) = self.questions.get_mut(self.focused_index) else { return };
        let cursor = q.cursor;
        let other_idx = q.options.len();

        if cursor == other_idx {
            q.typing_other = true;
            if !q.multi_select {
                q.selected.clear();
            }
            return;
        }

        if q.multi_select {
            if let Some(pos) = q.selected.iter().position(|&s| s == cursor) {
                _ = q.selected.remove(pos);
            } else {
                q.selected.push(cursor);
            }
            q.typing_other = false;
        } else {
            q.selected = vec![cursor];
            q.typing_other = false;
            q.other_text.clear();
        }
    }

    /// Handle Enter: select + advance (single), or advance (multi).
    pub fn handle_enter(&mut self) {
        let Some(q) = self.questions.get(self.focused_index) else { return };
        let selected_empty = q.selected.is_empty();
        let typing_other = q.typing_other;

        if !q.multi_select && selected_empty && !typing_other {
            self.toggle_selection();
        }

        if self.focused_index < self.questions.len().saturating_sub(1) {
            self.focused_index = self.focused_index.saturating_add(1);
        }
        // Final submit is handled externally (not here)
    }

    /// Whether the current question has been answered.
    #[must_use]
    pub fn current_question_answered(&self) -> bool {
        let Some(q) = self.questions.get(self.focused_index) else { return false };
        !q.selected.is_empty() || (q.typing_other && !q.other_text.is_empty())
    }

    /// Whether ALL questions have been answered.
    #[must_use]
    pub fn all_answered(&self) -> bool {
        self.questions.iter().all(|q| !q.selected.is_empty() || (q.typing_other && !q.other_text.is_empty()))
    }

    /// Whether this is the last question.
    #[must_use]
    pub const fn is_last_question(&self) -> bool {
        self.focused_index >= self.questions.len().saturating_sub(1)
    }

    /// Go to previous question.
    pub const fn prev_question(&mut self) {
        if self.focused_index > 0 {
            self.focused_index = self.focused_index.saturating_sub(1);
        }
    }

    /// Go to next question (only if current is answered).
    pub fn next_question(&mut self) {
        if self.focused_index < self.questions.len().saturating_sub(1) && self.current_question_answered() {
            self.focused_index = self.focused_index.saturating_add(1);
        }
    }

    /// Type a character into the "Other" text field.
    pub fn type_char(&mut self, c: char) {
        if let Some(q) = self.questions.get_mut(self.focused_index)
            && q.typing_other
        {
            q.other_text.push(c);
        }
    }

    /// Backspace in the "Other" text field.
    pub fn backspace(&mut self) {
        if let Some(q) = self.questions.get_mut(self.focused_index)
            && q.typing_other
        {
            _ = q.other_text.pop();
        }
    }

    /// Format all answers as YAML for the answer message.
    #[must_use]
    pub fn format_answers_yaml(&self) -> String {
        let mut lines = Vec::new();
        for q in &self.questions {
            lines.push(format!("{}:", q.header));
            let selected_labels: Vec<&str> =
                q.selected.iter().filter_map(|&idx| q.options.get(idx).map(|o| o.label.as_str())).collect();
            if !selected_labels.is_empty() {
                for label in &selected_labels {
                    lines.push(format!("  - {label}"));
                }
            }
            if q.typing_other && !q.other_text.is_empty() {
                let escaped =
                    q.other_text.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r");
                lines.push(format!("  other: \"{escaped}\""));
            }
            if selected_labels.is_empty() && (!q.typing_other || q.other_text.is_empty()) {
                lines.push("  (no answer)".to_owned());
            }
        }
        lines.join("\n")
    }
}
