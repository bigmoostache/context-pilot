use serde::{Deserialize, Serialize};

use cp_base::state::runtime::State;

// =============================================================================
// Enums
// =============================================================================

/// Thread turn status — who needs to act next.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadStatus {
    /// The AI's turn — thread has user input awaiting response.
    MyTurn,
    /// The user's turn — AI has responded, waiting for user.
    #[default]
    TheirTurn,
}

impl std::fmt::Display for ThreadStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::MyTurn => write!(f, "MY_TURN"),
            Self::TheirTurn => write!(f, "THEIR_TURN"),
        }
    }
}

/// Who authored a thread message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadAuthor {
    /// Message from the human user.
    #[default]
    User,
    /// Message from the AI assistant.
    Assistant,
}

impl std::fmt::Display for ThreadAuthor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThreadMessage {
    /// Who wrote this message.
    pub author: ThreadAuthor,
    /// Markdown text content (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Attached file path reference (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// Creation timestamp (epoch ms).
    pub timestamp: u64,
    /// Whether the AI has acknowledged (seen via `Read`) this message.
    /// AI-authored messages are acknowledged on creation. User messages
    /// start unacknowledged and become acknowledged when `Read` is called.
    #[serde(default = "default_true")]
    pub acknowledged: bool,
    /// True when this is an auto-generated **tool-activity trace** — a
    /// lightweight `{verb · tool — intent}` line appended on every tool call
    /// while the AI is focused on the thread (see the tool pipeline). Auto
    /// messages are *hidden from the AI's own context* (skipped in
    /// [`build_panel_content`](crate::tools)) so the model never re-reads its
    /// own action log, and are rendered as a **collapsible group** in the web
    /// UI and TUI rather than as normal bubbles. They never change a thread's
    /// turn/focus. Defaults to `false` (back-compat with pre-feature data).
    #[serde(default)]
    pub auto: bool,
}

impl ThreadMessage {
    /// A user-authored text message, unacknowledged, stamped now.
    #[must_use]
    pub fn user(content: String) -> Self {
        Self {
            author: ThreadAuthor::User,
            content: Some(content),
            file_path: None,
            timestamp: cp_base::panels::now_ms(),
            acknowledged: false,
            auto: false,
        }
    }

    /// An assistant-authored auto **tool-activity trace** — acknowledged (so it
    /// never flips turn/unread) and flagged [`auto`](Self::auto) (hidden from the
    /// agent's own context, rendered as a collapsible run). Stamped now.
    #[must_use]
    pub fn auto_trace(content: String) -> Self {
        Self {
            author: ThreadAuthor::Assistant,
            content: Some(content),
            file_path: None,
            timestamp: cp_base::panels::now_ms(),
            acknowledged: true,
            auto: true,
        }
    }
}

/// Where a branched thread came from: the parent thread and the message it was
/// cut at (inclusive). Set only on threads created by [`ThreadsState::branch`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadOrigin {
    /// Id of the parent thread at branch time (may since have been deleted).
    pub thread_id: String,
    /// Epoch-ms timestamp of the last parent message copied into the branch.
    pub message_ts: u64,
}

/// A parallel discussion/work topic thread.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// Pause flag — paused threads suppress `MY_TURN` idle notifications so
    /// the AI does not nag about them, but remain visible and fully functional.
    /// The user sets this when they are still composing input and do not want
    /// the agent to act yet.
    #[serde(default)]
    pub paused: bool,
    /// Parent thread + branch point when this thread was branched out of
    /// another one (`None` for a thread created from scratch). Defaults to
    /// `None` (back-compat with pre-feature data).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<ThreadOrigin>,
}

impl Thread {
    /// A fresh empty thread: caller's turn is `TheirTurn` (user types first),
    /// not archived, not paused, stamped now.
    #[must_use]
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            status: ThreadStatus::TheirTurn,
            messages: vec![],
            created_at: cp_base::panels::now_ms(),
            archived: false,
            paused: false,
            origin: None,
        }
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
        self.threads.iter().any(|t| !t.archived && !t.paused && t.status == ThreadStatus::MyTurn)
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
        self.threads.iter().enumerate().filter(|entry| entry.1.archived == archived).map(|entry| entry.0).collect()
    }

    /// Branch a new thread out of `source_id`: a fresh thread named `name`
    /// whose history is a copy of the parent's messages up to and including the
    /// one stamped `at_ts`. Returns the new thread's id.
    ///
    /// Copied messages keep their original timestamps (still unique within the
    /// branch, so `DeleteMessage` works on it) and are all marked acknowledged —
    /// the branch starts from context the agent has already seen. The branch is
    /// a plain `TheirTurn` thread (never archived/paused, even if the parent
    /// is) recording its parent in [`Thread::origin`]. Nothing else the parent
    /// owns (todos, scratchpad) is copied: those only exist as a *current*
    /// snapshot, which would leak whatever happened after the branch point.
    ///
    /// # Errors
    ///
    /// Returns a human-readable message when the source thread does not exist
    /// or holds no message stamped `at_ts`; the state is left untouched.
    pub fn branch(&mut self, source_id: &str, at_ts: u64, name: &str) -> Result<String, String> {
        let source =
            self.threads.iter().find(|t| t.id == source_id).ok_or_else(|| format!("thread {source_id} not found"))?;
        let cut = source
            .messages
            .iter()
            .position(|m| m.timestamp == at_ts)
            .ok_or_else(|| format!("no message with ts={at_ts} in thread {source_id}"))?;
        let messages: Vec<ThreadMessage> = source
            .messages
            .iter()
            .take(cut.saturating_add(1))
            .map(|m| ThreadMessage { acknowledged: true, ..m.clone() })
            .collect();

        let id = format!("T{}", self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        let mut thread = Thread::new(id.clone(), name.to_owned());
        thread.messages = messages;
        thread.origin = Some(ThreadOrigin { thread_id: source_id.to_owned(), message_ts: at_ts });
        self.threads.push(thread);
        Ok(id)
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
    /// Escalation severity counter. Increments on each tool completion while
    /// the AI is unfocused with a `MY_TURN` thread pending; reset on focus.
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
}

impl Default for FocusState {
    fn default() -> Self {
        Self::new()
    }
}

impl FocusState {
    /// Initial focus state: unfocused, no escalation.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            focused_thread_id: None,
            escalation_level: 0,
            selected_thread_idx: 0,
            creating_thread: false,
            confirming_archive: false,
            viewing_archived: false,
            last_read_count: std::collections::BTreeMap::new(),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A message by `author` stamped `ts`, unacknowledged.
    fn msg(author: ThreadAuthor, text: &str, ts: u64) -> ThreadMessage {
        ThreadMessage {
            author,
            content: Some(text.to_owned()),
            file_path: None,
            timestamp: ts,
            acknowledged: false,
            auto: false,
        }
    }

    /// State holding one parent thread `T1` with four messages (ts 10..=40),
    /// archived + paused + `MyTurn` so the test can check none of it carries over.
    fn parent_state() -> ThreadsState {
        let mut ts = ThreadsState::new();
        let mut parent = Thread::new("T1".to_owned(), "Parent".to_owned());
        parent.messages = vec![
            msg(ThreadAuthor::User, "q1", 10),
            msg(ThreadAuthor::Assistant, "a1", 20),
            msg(ThreadAuthor::User, "q2", 30),
            msg(ThreadAuthor::Assistant, "a2", 40),
        ];
        parent.status = ThreadStatus::MyTurn;
        parent.archived = true;
        parent.paused = true;
        ts.threads.push(parent);
        ts.next_id = 2;
        ts
    }

    #[test]
    fn branch_copies_history_up_to_and_including_the_branch_point() {
        let mut ts = parent_state();
        let id = ts.branch("T1", 20, "Alt").unwrap();
        assert_eq!(id, "T2");
        assert_eq!(ts.next_id, 3);

        let branch = ts.threads.iter().find(|t| t.id == "T2").unwrap();
        let texts: Vec<_> = branch.messages.iter().map(|m| m.content.as_deref().unwrap_or("")).collect();
        assert_eq!(texts, ["q1", "a1"]);
        assert_eq!(branch.messages.iter().map(|m| m.timestamp).collect::<Vec<_>>(), [10, 20]);
        assert!(branch.messages.iter().all(|m| m.acknowledged));
        assert_eq!(branch.origin, Some(ThreadOrigin { thread_id: "T1".to_owned(), message_ts: 20 }));
    }

    #[test]
    fn branch_is_a_fresh_active_thread_and_leaves_the_parent_untouched() {
        let mut ts = parent_state();
        let id = ts.branch("T1", 20, "Alt").unwrap();

        let branch = ts.threads.iter().find(|t| t.id == id).unwrap();
        assert_eq!(branch.name, "Alt");
        assert_eq!(branch.status, ThreadStatus::TheirTurn);
        assert!(!branch.archived);
        assert!(!branch.paused);

        let parent = ts.threads.iter().find(|t| t.id == "T1").unwrap();
        assert_eq!(parent.messages.len(), 4);
        assert!(parent.messages.iter().all(|m| !m.acknowledged));
    }

    #[test]
    fn branch_at_last_message_copies_everything() {
        let mut ts = parent_state();
        let id = ts.branch("T1", 40, "Copy").unwrap();
        let branch = ts.threads.iter().find(|t| t.id == id).unwrap();
        assert_eq!(branch.messages.len(), 4);
    }

    #[test]
    fn branch_rejects_unknown_thread_or_message() {
        let mut ts = parent_state();
        let unknown_thread = ts.branch("T9", 20, "x").unwrap_err();
        assert!(unknown_thread.contains("T9"));
        let unknown_message = ts.branch("T1", 25, "x").unwrap_err();
        assert!(unknown_message.contains("ts=25"));
        assert_eq!(ts.threads.len(), 1);
        assert_eq!(ts.next_id, 2);
    }

    #[test]
    fn origin_round_trips_and_defaults_to_none() {
        let legacy: Thread =
            serde_json::from_str(r#"{"id":"T1","name":"n","status":"TheirTurn","messages":[],"created_at":1}"#)
                .unwrap();
        assert_eq!(legacy.origin, None);
        assert!(!serde_json::to_string(&legacy).unwrap().contains("origin"));

        let mut ts = parent_state();
        let id = ts.branch("T1", 30, "b").unwrap();
        let branch = ts.threads.iter().find(|t| t.id == id).unwrap();
        let back: Thread = serde_json::from_str(&serde_json::to_string(branch).unwrap()).unwrap();
        assert_eq!(back.origin, branch.origin);
    }
}
