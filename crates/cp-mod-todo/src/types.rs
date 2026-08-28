use cp_base::config::accessors::icons;
use cp_base::state::runtime::State;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Todo item status.
///
/// Four states (thread-owned tasks rework): `Planned` (replaces the old
/// `Pending`), `InProgress`, `Done`, and `Cancelled` — the soft-delete. A
/// cancelled item is hidden from the panel and excluded from every count and
/// from `check_done_allowed`; it is never hard-removed except when its owning
/// thread is deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    #[default]
    /// Not started.
    Planned, // ' '
    /// Work in progress.
    InProgress, // '~'
    /// Completed.
    Done, // 'x'
    /// Soft-deleted — hidden from the panel, excluded from all counts.
    Cancelled, // '/'
}

impl TodoStatus {
    /// Theme icon for this status (e.g., "○ ", "◐ ", "● ").
    ///
    /// `Cancelled` uses a literal `✕` — there is no theme glyph for it (it is
    /// never rendered in the panel, so a config-schema entry would be dead).
    #[must_use]
    pub fn icon(self) -> String {
        match self {
            Self::Planned => icons::todo_pending(),
            Self::InProgress => icons::todo_in_progress(),
            Self::Done => icons::todo_done(),
            Self::Cancelled => "\u{2715}".to_owned(),
        }
    }
}

impl FromStr for TodoStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            " " | "planned" | "pending" => Ok(Self::Planned),
            "~" | "in_progress" => Ok(Self::InProgress),
            "x" | "X" | "done" => Ok(Self::Done),
            "/" | "cancelled" => Ok(Self::Cancelled),
            _ => Err(()),
        }
    }
}

/// A todo item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    /// Todo ID (X1, X2, ...)
    pub id: String,
    /// Owning thread id (compulsory, thread-owned tasks rework). An item with
    /// no owning thread cannot exist; legacy items lacking one are purged on
    /// load. Serialized unconditionally (no skip) — it is a foreign key.
    #[serde(default)]
    pub thread_id: String,
    /// Parent todo ID (for nesting). Its parent MUST share the same `thread_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Todo name/title
    pub name: String,
    /// Detailed description
    #[serde(default)]
    pub description: String,
    /// Status: pending, `in_progress`, done
    #[serde(default)]
    pub status: TodoStatus,
}

/// Module-owned state for the Todo module
#[derive(Debug)]
pub struct TodoState {
    /// All todo items (top-level + nested children).
    pub todos: Vec<TodoItem>,
    /// Counter for generating unique IDs (X1, X2, ...).
    pub next_todo_id: usize,
    /// Injected focused-thread id for panel scoping (thread-owned tasks
    /// rework). The main crate stamps the current focused thread id here on
    /// focus change; the panel renders only items whose `thread_id` matches.
    /// **Transient** — never serialized (see `save_module_data`).
    pub focus_filter: Option<String>,
    /// The thread most recently work-hygiene-nudged (FR11 fire-once tracking).
    /// The main crate sets this after emitting a nudge so it does not re-fire on
    /// every tool call; it clears when the condition clears or focus moves.
    /// **Transient** — never serialized.
    pub nudged_thread: Option<String>,
}

impl Default for TodoState {
    fn default() -> Self {
        Self::new()
    }
}

impl TodoState {
    /// Create an empty todo state with ID counter at 1.
    #[must_use]
    pub const fn new() -> Self {
        Self { todos: vec![], next_todo_id: 1, focus_filter: None, nudged_thread: None }
    }

    /// Get shared ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Get mutable ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }
}
