//! Message and context icons from the active theme.

use super::active_theme;
use crate::config::normalize_icon;

/// User message icon (e.g., "⚔ ").
#[must_use]
pub fn msg_user() -> String {
    normalize_icon(&active_theme().messages.user)
}
/// Assistant message icon (e.g., "🐉 ").
#[must_use]
pub fn msg_assistant() -> String {
    normalize_icon(&active_theme().messages.assistant)
}
/// Tool-call message icon.
#[must_use]
pub fn msg_tool_call() -> String {
    normalize_icon(&active_theme().messages.tool_call)
}
/// Tool-result message icon.
#[must_use]
pub fn msg_tool_result() -> String {
    normalize_icon(&active_theme().messages.tool_result)
}
/// Error message icon.
#[must_use]
pub fn msg_error() -> String {
    normalize_icon(&active_theme().messages.error)
}
/// Status icon for messages included in full.
#[must_use]
pub fn status_full() -> String {
    normalize_icon(&active_theme().status.full)
}
/// Status icon for deleted/detached messages.
#[must_use]
pub fn status_deleted() -> String {
    normalize_icon(&active_theme().status.deleted)
}
/// Todo icon for pending items.
#[must_use]
pub fn todo_pending() -> String {
    normalize_icon(&active_theme().todo.pending)
}
/// Todo icon for in-progress items.
#[must_use]
pub fn todo_in_progress() -> String {
    normalize_icon(&active_theme().todo.in_progress)
}
/// Todo icon for completed items.
#[must_use]
pub fn todo_done() -> String {
    normalize_icon(&active_theme().todo.done)
}
