//! Icon accessors loaded from the active YAML theme, normalized to 2 cells.

use crate::infra::config::{active_theme, normalize_icon};

/// Icon for user messages (normalized to 2 cells).
pub(crate) fn msg_user() -> String {
    normalize_icon(&active_theme().messages.user)
}
/// Icon for assistant messages (normalized to 2 cells).
pub(crate) fn msg_assistant() -> String {
    normalize_icon(&active_theme().messages.assistant)
}
/// Icon for tool call messages (normalized to 2 cells).
pub(crate) fn msg_tool_call() -> String {
    normalize_icon(&active_theme().messages.tool_call)
}
/// Icon for tool result messages (normalized to 2 cells).
pub(crate) fn msg_tool_result() -> String {
    normalize_icon(&active_theme().messages.tool_result)
}
/// Icon for error messages (normalized to 2 cells).
pub(crate) fn msg_error() -> String {
    normalize_icon(&active_theme().messages.error)
}

/// Icon for full status indicator (normalized to 2 cells).
pub(crate) fn status_full() -> String {
    normalize_icon(&active_theme().status.full)
}
/// Icon for deleted status indicator (normalized to 2 cells).
pub(crate) fn status_deleted() -> String {
    normalize_icon(&active_theme().status.deleted)
}
