use serde::{Deserialize, Serialize};

use cp_base::state::runtime::State;

// =============================================================================
// Enums
// =============================================================================

/// Thread turn status — who needs to act next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadStatus {
    /// The AI's turn — thread has user input awaiting response.
    MyTurn,
    /// The user's turn — AI has responded, waiting for user.
    TheirTurn,
}

impl std::fmt::Display for ThreadStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MyTurn => write!(f, "MY_TURN"),
            Self::TheirTurn => write!(f, "THEIR_TURN"),
        }
    }
}

/// Who authored a thread message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadAuthor {
    /// Message from the human user.
    User,
    /// Message from the AI assistant.
    Assistant,
}

impl std::fmt::Display for ThreadAuthor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Assistant => write!(f, "assistant"),
        }
    }
}

// =============================================================================
// Structs
// =============================================================================

/// A single message within a thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadMessage {
    /// Who wrote this message.
    pub author: ThreadAuthor,
    /// Markdown text content (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Attached file path reference (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// Embedded question form (if any). Stored as raw JSON for now;
    /// Phase 7 introduces a typed `ThreadQuestion` struct.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<serde_json::Value>,
    /// Creation timestamp (epoch ms).
    pub timestamp: u64,
}

/// A parallel discussion/work topic thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    /// Short unique identifier (e.g. "T1", "T2").
    pub id: String,
    /// Free-text label chosen by the user.
    pub name: String,
    /// Whose turn it is.
    pub status: ThreadStatus,
    /// Ordered list of messages.
    pub messages: Vec<ThreadMessage>,
    /// Creation timestamp (epoch ms).
    pub created_at: u64,
}

// =============================================================================
// Question form types — interactive question forms embedded in thread messages
// =============================================================================

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

impl ThreadQuestionForm {
    /// Parse a question form from a thread message's JSON question field.
    ///
    /// Returns `None` if the JSON is malformed or empty.
    #[must_use]
    pub fn from_json(thread_id: &str, json: &serde_json::Value) -> Option<Self> {
        let arr = json.as_array()?;
        if arr.is_empty() {
            return None;
        }
        let questions: Vec<ThreadQuestion> = arr
            .iter()
            .filter_map(|q| {
                let header = q.get("header")?.as_str()?.to_owned();
                let text = q.get("question")?.as_str()?.to_owned();
                let multi_select = q.get("multiSelect").and_then(serde_json::Value::as_bool).unwrap_or(false);
                let options = q.get("options")?.as_array()?.iter().filter_map(|o| {
                    Some(ThreadQuestionOption {
                        label: o.get("label")?.as_str()?.to_owned(),
                        description: o.get("description")?.as_str()?.to_owned(),
                    })
                }).collect();
                Some(ThreadQuestion {
                    header, text, options, multi_select,
                    cursor: 0, selected: Vec::new(), typing_other: false, other_text: String::new(),
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
            let selected_labels: Vec<&str> = q
                .selected
                .iter()
                .filter_map(|&idx| q.options.get(idx).map(|o| o.label.as_str()))
                .collect();
            if !selected_labels.is_empty() {
                for label in &selected_labels {
                    lines.push(format!("  - {label}"));
                }
            }
            if q.typing_other && !q.other_text.is_empty() {
                lines.push(format!("  other: \"{}\"", q.other_text));
            }
            if selected_labels.is_empty() && (!q.typing_other || q.other_text.is_empty()) {
                lines.push("  (no answer)".to_owned());
            }
        }
        lines.join("\n")
    }
}

// =============================================================================
// Module State — shared (is_global=true)
// =============================================================================

/// Shared thread state, persisted via `save_module_data`.
#[derive(Debug)]
pub struct ThreadsState {
    /// All active threads.
    pub threads: Vec<Thread>,
    /// Counter for generating unique thread IDs (T1, T2, ...).
    pub next_id: u32,
}

impl Default for ThreadsState {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadsState {
    /// Create an empty threads state with ID counter at 1.
    #[must_use]
    pub const fn new() -> Self {
        Self { threads: vec![], next_id: 1 }
    }

    /// Get shared ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if `ThreadsState` was never inserted into state.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Get mutable ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if `ThreadsState` was never inserted into state.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }

    /// Returns true if any thread is in `MyTurn` status.
    #[must_use]
    pub fn has_my_turn_threads(&self) -> bool {
        self.threads.iter().any(|t| t.status == ThreadStatus::MyTurn)
    }
}

// =============================================================================
// Focus State — per-worker (save_worker_data / load_worker_data)
// =============================================================================

/// Per-worker focus tracking for thread enforcement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusState {
    /// Which thread the AI is currently focused on (None = unfocused).
    pub focused_thread_id: Option<String>,
    /// Remaining tool calls in the dangling phase after `Send` clears focus.
    /// Starts at 5 after Send, decremented on each non-exempt tool call.
    /// Negative values mean the dangling phase has expired.
    pub dangling_remaining: i32,
    /// Escalation severity counter. Increments after dangling phase expires
    /// if the AI still hasn't focused on a thread.
    pub escalation_level: u32,
    /// Index of the currently selected thread in the TUI threads view.
    /// Used for navigation (Tab/Shift+Tab) and message area display.
    #[serde(default)]
    pub selected_thread_idx: usize,
    /// When true, the input field is being used to name a new thread.
    /// Set by pressing 'n' in Threads view, cleared on Enter or Esc.
    #[serde(default)]
    pub creating_thread: bool,
    /// When true, the user is confirming thread archive/deletion.
    /// Set by pressing 'a' in Threads view, cleared on 'y' (confirm) or any other key.
    #[serde(default)]
    pub confirming_archive: bool,
    /// Per-thread last-read message count, keyed by thread ID.
    /// Used for unread indicators — a thread is "unread" when
    /// `messages.len() > last_read_count[thread_id]`.
    #[serde(default)]
    pub last_read_count: std::collections::BTreeMap<String, usize>,
    /// Thread ID for which we last sent an idle+`MY_TURN` notification.
    /// Used for debouncing — prevents spamming the same notification
    /// every tick. Cleared when the thread transitions to `THEIR_TURN`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notified_my_turn_id: Option<String>,
    /// Active question form state for thread-embedded questions.
    /// Set when the selected thread has pending questions that the user
    /// hasn't answered yet. Cleared on submit or dismiss.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_question: Option<ThreadQuestionForm>,
}

impl Default for FocusState {
    fn default() -> Self {
        Self::new()
    }
}

impl FocusState {
    /// Initial focus state: unfocused, no dangling phase, no escalation.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            focused_thread_id: None,
            dangling_remaining: 0,
            escalation_level: 0,
            selected_thread_idx: 0,
            creating_thread: false,
            confirming_archive: false,
            last_read_count: std::collections::BTreeMap::new(),
            notified_my_turn_id: None,
            active_question: None,
        }
    }

    /// Get shared ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if `FocusState` was never inserted into state.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Get mutable ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if `FocusState` was never inserted into state.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }

    /// Mark the currently selected thread as fully read.
    /// Updates `last_read_count` to the thread's current message count.
    pub fn mark_selected_read(state: &mut State) {
        let threads = ThreadsState::get(state);
        let focus = Self::get(state);
        let idx = focus.selected_thread_idx.min(threads.threads.len().saturating_sub(1));
        if let Some(thread) = threads.threads.get(idx) {
            let tid = thread.id.clone();
            let count = thread.messages.len();
            let _prev = Self::get_mut(state).last_read_count.insert(tid, count);
        }
    }
}
