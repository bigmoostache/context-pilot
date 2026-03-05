use cp_base::config::accessors::icons;
use cp_base::state::runtime::State;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Todo item status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    #[default]
    /// Not started.
    Pending, // ' '
    /// Work in progress.
    InProgress, // '~'
    /// Completed.
    Done, // 'x'
}

impl TodoStatus {
    /// Theme icon for this status (e.g., "○ ", "◐ ", "● ").
    #[must_use]
    pub fn icon(self) -> String {
        match self {
            Self::Pending => icons::todo_pending(),
            Self::InProgress => icons::todo_in_progress(),
            Self::Done => icons::todo_done(),
        }
    }
}

impl FromStr for TodoStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            " " | "pending" => Ok(Self::Pending),
            "~" | "in_progress" => Ok(Self::InProgress),
            "x" | "X" | "done" => Ok(Self::Done),
            _ => Err(()),
        }
    }
}

/// A todo item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    /// Todo ID (X1, X2, ...)
    pub id: String,
    /// Parent todo ID (for nesting)
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
        Self { todos: vec![], next_todo_id: 1 }
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

    /// Check if there are any pending or in-progress todos
    #[must_use]
    pub fn has_incomplete_todos(&self) -> bool {
        self.todos.iter().any(|t| matches!(t.status, TodoStatus::Pending | TodoStatus::InProgress))
    }

    /// Get a summary of incomplete todos for spine auto-continuation messages
    #[must_use]
    pub fn incomplete_todos_summary(&self) -> Vec<String> {
        self.todos
            .iter()
            .filter(|t| matches!(t.status, TodoStatus::Pending | TodoStatus::InProgress))
            .map(|t| format!("[{}] {} — {}", t.id, t.status.icon(), t.name))
            .collect()
    }
}
