use serde::{Deserialize, Serialize};

use cp_base::state::runtime::State;

/// Per-thread message files (`threads/<id>.json`), written only when dirty.
pub mod persist;

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
    /// True once an **inline push notification** has been emitted for this
    /// message while the agent was mid-stream (Branch B of the incoming-message
    /// behavior). The push path sets this the first time it nudges the agent
    /// about the message, so a long-running stream never re-pushes the same
    /// message on every poll tick. It is message-level and serialized so the
    /// guard survives reloads. Independent of [`acknowledged`](Self::acknowledged):
    /// a message can be pushed (agent nudged mid-stream) yet still unacknowledged
    /// (not pulled into context via `Read`) until the agent actually reads it.
    /// Defaults to `false` (back-compat with pre-feature data).
    #[serde(default)]
    pub has_been_pushed: bool,
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
            has_been_pushed: false,
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
            has_been_pushed: false,
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
    /// Ordered list of messages. Persisted in `threads/<id>.json`, not in
    /// `config.json` (see [`persist`]); empty after a slim deserialize.
    #[serde(default)]
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
    /// Which thread the human has *drilled into* in the TUI (`None` = showing the
    /// thread list). Pure view state (G3): it selects whose full panel view is
    /// painted, and never moves execution — the renderer swaps the drilled
    /// thread's parked runtime in only for the duration of one paint, then
    /// restores the resident (Model 2: a human glance must not disturb the
    /// agent's work). Defaults to `None` (back-compat; byte-identical until set).
    #[serde(default)]
    pub drilled_thread_id: Option<String>,
    /// Index of the currently selected thread in the TUI threads view.
    /// Used for navigation (Tab/Shift+Tab) and message area display.
    #[serde(default)]
    pub selected_thread_idx: usize,
    /// When true, the input field is being used to name a new thread.
    /// Set by pressing 'n' in Threads view, cleared on Enter or Esc.
    #[serde(default)]
    pub creating_thread: bool,
    /// When true, an archive/restore is *armed*: a first Ctrl+X was pressed and
    /// the TUI is waiting for a confirming second Ctrl+X. Cleared on confirm, on
    /// any other (non-Ctrl) key, or implicitly once the 2-second window lapses.
    #[serde(default)]
    pub confirming_archive: bool,
    /// Wall-clock ms ([`now_ms`](cp_base::panels::now_ms)) of the first Ctrl+X
    /// that armed the pending archive. The confirming second Ctrl+X only counts
    /// while `now - archive_armed_at_ms <= 2000`; a later press re-arms instead.
    #[serde(default)]
    pub archive_armed_at_ms: u64,
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
    /// Thread currently under the list cursor and the ms it got there. A thread
    /// is only marked read after [`READ_DWELL_MS`] of continuous selection, so
    /// arrowing past rows leaves their unread marker intact. Transient.
    #[serde(skip)]
    pub read_dwell: Option<(String, u64)>,
    /// Draft name typed on the virtual "+ New Thread" row. Its own textarea
    /// (not a thread's composer) so it survives navigation and reloads.
    #[serde(default)]
    pub new_thread_title: cp_base::state::runtime::textarea::TextArea,
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
            drilled_thread_id: None,
            selected_thread_idx: 0,
            creating_thread: false,
            confirming_archive: false,
            archive_armed_at_ms: 0,
            viewing_archived: false,
            last_read_count: std::collections::BTreeMap::new(),
            read_dwell: None,
            new_thread_title: cp_base::state::runtime::textarea::TextArea::new(),
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

    /// Per-tick read tracking for the Threads view: restart the dwell clock
    /// when the selected thread changes; once it has stayed selected for
    /// [`READ_DWELL_MS`], mark it read (and keep doing so while it stays, so
    /// replies arriving under the cursor count as seen).
    pub fn tick_read_dwell(state: &mut State, now_ms: u64) {
        let selected = (state.view_mode == cp_base::state::data::config::ViewMode::Threads)
            .then(|| {
                let focus = Self::get(state);
                let threads = ThreadsState::get(state);
                let visible = threads.visible_indices(focus.viewing_archived);
                let real_idx = *visible.get(focus.selected_thread_idx)?;
                threads.threads.get(real_idx).map(|t| t.id.clone())
            })
            .flatten();
        let dwell = &mut Self::get_mut(state).read_dwell;
        let since = dwell.as_ref().filter(|d| selected.as_deref() == Some(d.0.as_str())).map(|d| d.1);
        let dwelled = since.map_or_else(
            || {
                *dwell = selected.map(|id| (id, now_ms));
                false
            },
            |start| now_ms.saturating_sub(start) >= READ_DWELL_MS,
        );
        if dwelled {
            Self::mark_selected_read(state);
        }
    }
}

/// Continuous selection time before a thread's messages count as user-read.
pub const READ_DWELL_MS: u64 = 2_000;

#[cfg(test)]
mod tests;
