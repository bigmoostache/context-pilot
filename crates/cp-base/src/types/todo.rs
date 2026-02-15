use crate::constants::icons;
use serde::{Deserialize, Serialize};

/// Todo item status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    #[default]
    Pending, // ' '
    InProgress, // '~'
    Done,       // 'x'
}

impl TodoStatus {
    pub fn icon(&self) -> String {
        match self {
            TodoStatus::Pending => icons::todo_pending(),
            TodoStatus::InProgress => icons::todo_in_progress(),
            TodoStatus::Done => icons::todo_done(),
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            " " | "pending" => Some(TodoStatus::Pending),
            "~" | "in_progress" => Some(TodoStatus::InProgress),
            "x" | "X" | "done" => Some(TodoStatus::Done),
            _ => None,
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
    /// Status: pending, in_progress, done
    #[serde(default)]
    pub status: TodoStatus,
}
