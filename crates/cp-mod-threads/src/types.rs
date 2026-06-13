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
}
