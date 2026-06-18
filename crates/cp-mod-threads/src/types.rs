use serde::{Deserialize, Serialize};

use cp_base::state::runtime::State;

use crate::questions::ThreadQuestionForm;

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

/// Serde helper — returns `true`.
const fn default_true() -> bool {
    true
}

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
    /// Whether the AI has acknowledged (seen via `Read`) this message.
    /// AI-authored messages are acknowledged on creation. User messages
    /// start unacknowledged and become acknowledged when `Read` is called.
    #[serde(default = "default_true")]
    pub acknowledged: bool,
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
    /// Soft-delete flag — archived threads are hidden from the active list
    /// but retained in state so the web frontend can display and restore them.
    #[serde(default)]
    pub archived: bool,
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
    /// Pre-rendered panel content for the LLM. Only updated by `Read`.
    /// Contains the thread list summary + focused thread's full conversation.
    pub panel_content: String,
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
        Self { threads: vec![], next_id: 1, panel_content: String::new() }
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

    /// Returns true if any *non-archived* thread is in `MyTurn` status.
    ///
    /// Archived threads are invisible to the LLM (T9): they never trigger
    /// `MY_TURN` idle nudges, never appear in context. Restoring a thread
    /// makes it count again.
    #[must_use]
    pub fn has_my_turn_threads(&self) -> bool {
        self.threads.iter().any(|t| !t.archived && t.status == ThreadStatus::MyTurn)
    }

    /// Indices into [`Self::threads`] of the threads whose `archived` flag
    /// matches `archived`, in storage order.
    ///
    /// The TUI thread-centered view shows one subset at a time — the active
    /// list (`archived = false`) or the archived list (`archived = true`,
    /// toggled by Ctrl+U). Selection (`selected_thread_idx`) is a position
    /// **into this filtered slice**, resolved back to a real index here so a
    /// soft-deleted thread keeps its place in storage without polluting the
    /// visible list.
    #[must_use]
    pub fn visible_indices(&self, archived: bool) -> Vec<usize> {
        self.threads
            .iter()
            .enumerate()
            .filter(|(_, t)| t.archived == archived)
            .map(|(i, _)| i)
            .collect()
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
    /// When true, the TUI thread-centered view shows the *archived* threads
    /// instead of the active ones (toggled by Ctrl+U). Selection indexes into
    /// the matching filtered slice ([`ThreadsState::visible_indices`]); the
    /// virtual "+ New Thread" entry only appears in the active (non-archived)
    /// view.
    #[serde(default)]
    pub viewing_archived: bool,
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
            viewing_archived: false,
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
    ///
    /// `selected_thread_idx` is a position into the visible slice for the
    /// current view, so it is resolved through [`ThreadsState::visible_indices`]
    /// to the real storage index before marking.
    pub fn mark_selected_read(state: &mut State) {
        let threads = ThreadsState::get(state);
        let focus = Self::get(state);
        let visible = threads.visible_indices(focus.viewing_archived);
        let Some(&real_idx) = visible.get(focus.selected_thread_idx) else {
            return;
        };
        if let Some(thread) = threads.threads.get(real_idx) {
            let tid = thread.id.clone();
            let count = thread.messages.len();
            let _prev = Self::get_mut(state).last_read_count.insert(tid, count);
        }
    }
}
