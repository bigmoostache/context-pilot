//! Tool implementations for the logs module.
//!
//! Two tools:
//! - `log_create` — create timestamped entries with optional tags/importance
//! - `Close_conversation_history` — archive a history panel, extracting logs + memories

use cp_base::panels::mark_panels_dirty;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};
use cp_mod_memory::MEMORY_TLDR_MAX_TOKENS;
use cp_mod_memory::types::{MemoryImportance, MemoryItem, MemoryState};

use crate::types::{LogEntry, LogsState};

/// Helper: allocate a log ID, push a log entry (timestamped now) with metadata.
fn push_log(state: &mut State, content: String, importance: &str, tags: Vec<String>) {
    let ls = LogsState::get_mut(state);
    let id = format!("L{}", ls.next_log_id);
    ls.next_log_id = ls.next_log_id.saturating_add(1);

    let mut entry = LogEntry::new(id, content);
    entry.importance = importance.to_string();
    entry.tags = tags;
    ls.logs.push(entry);
}

/// Execute `log_create`: add one or more timestamped log entries.
pub(crate) fn execute_log_create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(entries) = tool.input.get("entries").and_then(|v| v.as_array()) else {
        return ToolResult::new(tool.id.clone(), "Missing required 'entries' array".to_string(), true);
    };

    if entries.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'entries' array".to_string(), true);
    }

    let mut count: usize = 0;
    for entry_obj in entries {
        if let Some(content) = entry_obj.get("content").and_then(|v| v.as_str())
            && !content.is_empty()
        {
            let importance = entry_obj.get("importance").and_then(|v| v.as_str()).unwrap_or("medium");
            let tags: Vec<String> = entry_obj
                .get("tags")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|t| t.as_str().map(String::from)).collect())
                .unwrap_or_default();

            push_log(state, content.to_string(), importance, tags);
            count = count.saturating_add(1);
        }
    }

    let mut result = ToolResult::new(tool.id.clone(), format!("Created {count} log(s)"), false);
    result.preserves_tempo = true;
    result
}

/// Execute `Close_conversation_history`: extract logs/memories and remove the panel.
///
/// The tool queue is auto-activated by `pre_flight` (`Verdict::activate_queue`)
/// before the pipeline's intercept check, so this call always arrives here
/// via a queue flush — never executed directly.
pub(crate) fn execute_close_conversation_history(tool: &ToolUse, state: &mut State) -> ToolResult {
    // 1. Validate the panel ID
    let panel_id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => {
            return ToolResult::new(tool.id.clone(), "Missing required 'id' parameter".to_string(), true);
        }
    };

    // Find the panel and verify it's a ConversationHistory
    let Some(panel_idx) = state.context.iter().position(|c| c.id == panel_id) else {
        return ToolResult::new(tool.id.clone(), format!("Panel '{panel_id}' not found"), true);
    };
    let Some(panel) = state.context.get(panel_idx) else {
        return ToolResult::new(tool.id.clone(), format!("Panel index {panel_idx} out of bounds"), true);
    };
    if panel.context_type.as_str() != Kind::CONVERSATION_HISTORY {
        return ToolResult::new(
            tool.id.clone(),
            format!("Panel '{}' is not a conversation history panel (type: {:?})", panel_id, panel.context_type),
            true,
        );
    }

    // 2. Extract the last message timestamp from the panel
    let last_msg_timestamp =
        panel.history_messages.as_ref().and_then(|msgs| msgs.last()).map_or(0, |msg| msg.timestamp_ms);

    // 3. Validate that logs are provided (at least one non-empty entry)
    let logs_array = tool.input.get("logs").and_then(|v| v.as_array());
    let has_logs = logs_array.is_some_and(|arr| {
        arr.iter().any(|e| e.get("content").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty()))
    });

    if !has_logs {
        return ToolResult::new(
            tool.id.clone(),
            "Cannot close conversation history without at least one log entry. \
             Provide 'logs' with meaningful entries to preserve context before closing."
                .to_string(),
            true,
        );
    }

    let mut output_parts = Vec::new();

    // 4. Create log entries (using panel's last message timestamp)
    if let Some(logs_array) = logs_array {
        let mut log_count: usize = 0;
        for log_obj in logs_array {
            if let Some(content) = log_obj.get("content").and_then(|v| v.as_str())
                && !content.is_empty()
            {
                let importance = log_obj.get("importance").and_then(|v| v.as_str()).unwrap_or("medium");
                let tags: Vec<String> = log_obj
                    .get("tags")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|t| t.as_str().map(String::from)).collect())
                    .unwrap_or_default();

                if last_msg_timestamp > 0 {
                    let ls = LogsState::get_mut(state);
                    let id = format!("L{}", ls.next_log_id);
                    ls.next_log_id = ls.next_log_id.saturating_add(1);
                    let mut entry = LogEntry::with_timestamp(id, content.to_string(), last_msg_timestamp);
                    entry.importance = importance.to_string();
                    entry.tags = tags;
                    ls.logs.push(entry);
                } else {
                    push_log(state, content.to_string(), importance, tags);
                }
                log_count = log_count.saturating_add(1);
            }
        }
        if log_count > 0 {
            output_parts.push(format!("Created {log_count} log(s)"));
        }
    }

    // 5. Create memory items
    if let Some(memories_array) = tool.input.get("memories").and_then(|v| v.as_array()) {
        let mut mem_count: usize = 0;
        for mem_obj in memories_array {
            if let Some(content) = mem_obj.get("content").and_then(|v| v.as_str())
                && !content.is_empty()
            {
                // Validate tl_dr length
                let tokens = estimate_tokens(content);
                if tokens > MEMORY_TLDR_MAX_TOKENS {
                    return ToolResult::new(
                        tool.id.clone(),
                        format!(
                            "Memory content too long for tl_dr: ~{tokens} tokens \
                             (max {MEMORY_TLDR_MAX_TOKENS}). Keep it short."
                        ),
                        true,
                    );
                }

                let importance = mem_obj.get("importance").and_then(|v| v.as_str()).unwrap_or("medium");

                let importance_level = match importance {
                    "low" => MemoryImportance::Low,
                    "high" => MemoryImportance::High,
                    "critical" => MemoryImportance::Critical,
                    _ => MemoryImportance::Medium,
                };

                let ms = MemoryState::get_mut(state);
                let id = format!("M{}", ms.next_memory_id);
                ms.next_memory_id = ms.next_memory_id.saturating_add(1);
                ms.memories.push(MemoryItem {
                    id,
                    tl_dr: content.to_string(),
                    contents: String::new(),
                    importance: importance_level,
                    labels: vec![],
                });
                mem_count = mem_count.saturating_add(1);
            }
        }
        if mem_count > 0 {
            output_parts.push(format!("Created {mem_count} memory(ies)"));
            mark_panels_dirty(state, Kind::MEMORY);
        }
    }

    // 6. Close the conversation history panel
    let panel_name = state.context.iter().find(|c| c.id == panel_id).map(|c| c.name.clone()).unwrap_or_default();
    state.context.retain(|c| c.id != panel_id);
    output_parts.push(format!("Closed {panel_id} ({panel_name})"));

    ToolResult::new(tool.id.clone(), output_parts.join("\n"), false)
}
