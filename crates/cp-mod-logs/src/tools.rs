//! Tool implementations for the logs module.
//!
//! Two tools:
//! - `log_create` — create timestamped entries with optional tags/importance
//! - `Close_conversation_history` — archive a history panel, extracting logs + memories

use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::{LogEntry, LogsState};

/// Helper: allocate a log ID, push a log entry (timestamped now) with metadata.
fn push_log(state: &mut State, content: String, importance: &str) {
    let ls = LogsState::get_mut(state);
    let id = format!("L{}", ls.next_log_id);
    ls.next_log_id = ls.next_log_id.saturating_add(1);

    let mut entry = LogEntry::new(id, content);
    importance.clone_into(&mut entry.importance);
    ls.logs.push(entry);
}

/// Execute `log_create`: add one or more timestamped log entries.
pub(crate) fn execute_log_create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("log_create");
    let Some(entries) = tool.input.get("entries").and_then(|v| v.as_array()) else {
        return ToolResult::new(tool.id.clone(), "Missing required 'entries' array".to_owned(), true);
    };

    if entries.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'entries' array".to_owned(), true);
    }

    let mut count: usize = 0;
    for entry_obj in entries {
        if let Some(content) = entry_obj.get("content").and_then(|v| v.as_str())
            && !content.is_empty()
        {
            let importance = entry_obj.get("importance").and_then(|v| v.as_str()).unwrap_or("medium");

            push_log(state, content.to_owned(), importance);
            count = count.saturating_add(1);
        }
    }

    let mut result = ToolResult::new(tool.id.clone(), format!("Created {count} log(s)"), false);
    result.preserves_tempo = true;
    result
}

/// Execute `Close_conversation_history`: extract logs and remove one or more panels.
///
/// The tool queue is auto-activated by `pre_flight` (`Verdict::activate_queue`)
/// before the pipeline's intercept check, so this call always arrives here
/// via a queue flush — never executed directly.
pub(crate) fn execute_close_conversation_history(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("close_history");
    // 1. Validate the panels array
    let Some(panels) = tool.input.get("panels").and_then(|v| v.as_array()) else {
        return ToolResult::new(tool.id.clone(), "Missing required 'panels' array".to_owned(), true);
    };

    if panels.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'panels' array".to_owned(), true);
    }

    let mut output_parts = Vec::new();

    for panel_obj in panels {
        let Some(panel_id) = panel_obj.get("panel_id").and_then(|v| v.as_str()) else {
            output_parts.push("Skipped entry: missing 'panel_id'".to_owned());
            continue;
        };

        // Find the panel and verify it's a ConversationHistory
        let Some(panel_idx) = state.context.iter().position(|c| c.id == panel_id) else {
            output_parts.push(format!("Panel '{panel_id}' not found"));
            continue;
        };
        let Some(panel) = state.context.get(panel_idx) else {
            output_parts.push(format!("Panel index {panel_idx} out of bounds"));
            continue;
        };
        if panel.context_type.as_str() != Kind::CONVERSATION_HISTORY {
            output_parts.push(format!(
                "Panel '{panel_id}' is not a conversation history panel (type: {:?})",
                panel.context_type
            ));
            continue;
        }

        // Extract the last message timestamp from the panel
        let last_msg_timestamp =
            panel.history_messages.as_ref().and_then(|msgs| msgs.last()).map_or(0, |msg| msg.timestamp_ms);

        // Create log entries from the panel's logs array (strings)
        let mut log_count: usize = 0;
        if let Some(logs_array) = panel_obj.get("logs").and_then(|v| v.as_array()) {
            for log_val in logs_array {
                if let Some(content) = log_val.as_str()
                    && !content.is_empty()
                {
                    if last_msg_timestamp > 0 {
                        let ls = LogsState::get_mut(state);
                        let id = format!("L{}", ls.next_log_id);
                        ls.next_log_id = ls.next_log_id.saturating_add(1);
                        let entry = LogEntry::with_timestamp(id, content.to_owned(), last_msg_timestamp);
                        ls.logs.push(entry);
                    } else {
                        push_log(state, content.to_owned(), "medium");
                    }
                    log_count = log_count.saturating_add(1);
                }
            }
        }

        // Close the conversation history panel
        let panel_name = state.context.iter().find(|c| c.id == panel_id).map(|c| c.name.clone()).unwrap_or_default();
        state.context.retain(|c| c.id != panel_id);
        output_parts.push(format!("Closed {panel_id} ({panel_name}) — {log_count} log(s)"));
    }

    ToolResult::new(tool.id.clone(), output_parts.join("\n"), false)
}
