use cp_base::panels::{mark_panels_dirty, now_ms};
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};
use cp_mod_memory::MEMORY_TLDR_MAX_TOKENS;
use cp_mod_memory::types::{MemoryImportance, MemoryItem, MemoryState};

use crate::panel;
use crate::types::{LogEntry, LogsState};

/// Helper: allocate a log ID and push a log entry (timestamped now)
fn push_log(state: &mut State, content: String) {
    let ls = LogsState::get_mut(state);
    let id = format!("L{}", ls.next_log_id);
    ls.next_log_id = ls.next_log_id.saturating_add(1);
    ls.logs.push(LogEntry::new(id, content));
}

/// Helper: allocate a log ID and push a log entry with an explicit timestamp
fn push_log_with_timestamp(state: &mut State, content: String, timestamp_ms: u64) {
    let ls = LogsState::get_mut(state);
    let id = format!("L{}", ls.next_log_id);
    ls.next_log_id = ls.next_log_id.saturating_add(1);
    ls.logs.push(LogEntry::with_timestamp(id, content, timestamp_ms));
}

/// Helper: touch logs panel to update `last_refresh_ms` and recalculate token count.
fn touch_logs_panel(state: &mut State) {
    let content = panel::LogsPanel::format_logs_tree(state);
    let token_count = estimate_tokens(&content);
    let now = now_ms();
    for ctx in &mut state.context {
        if ctx.context_type.as_str() == Kind::LOGS {
            ctx.token_count = token_count;
            ctx.last_refresh_ms = now;
        }
    }
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
            push_log(state, content.to_string());
            count = count.saturating_add(1);
        }
    }

    if count > 0 {
        touch_logs_panel(state);
    }

    ToolResult::new(tool.id.clone(), format!("Created {count} log(s)"), false)
}

/// Execute `log_summarize`: collapse multiple logs under a parent summary entry.
pub(crate) fn execute_log_summarize(tool: &ToolUse, state: &mut State) -> ToolResult {
    // Parse log_ids
    let log_ids: Vec<String> = match tool.input.get("log_ids").and_then(|v| v.as_array()) {
        Some(arr) => arr.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
        None => {
            return ToolResult::new(tool.id.clone(), "Missing required 'log_ids' array".to_string(), true);
        }
    };

    // Parse content
    let content = match tool.input.get("content").and_then(|v| v.as_str()) {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => {
            return ToolResult::new(tool.id.clone(), "Missing required 'content' parameter".to_string(), true);
        }
    };

    // Guardrail: minimum 4 entries
    if log_ids.len() < 4 {
        return ToolResult::new(
            tool.id.clone(),
            format!("Must summarize at least 4 logs, got {}", log_ids.len()),
            true,
        );
    }

    // Validate: all IDs exist and are top-level
    {
        let logs = &LogsState::get(state).logs;
        for id in &log_ids {
            match logs.iter().find(|l| l.id == *id) {
                None => {
                    return ToolResult::new(tool.id.clone(), format!("Log '{id}' not found"), true);
                }
                Some(log) => {
                    if log.parent_id.is_some() {
                        return ToolResult::new(
                            tool.id.clone(),
                            format!("Log '{id}' already has a parent — only top-level logs can be summarized"),
                            true,
                        );
                    }
                }
            }
        }
    }

    // Compute timestamp = max of children timestamps
    let max_timestamp = {
        let logs = &LogsState::get(state).logs;
        log_ids.iter().filter_map(|id| logs.iter().find(|l| l.id == *id)).map(|l| l.timestamp_ms).max().unwrap_or(0)
    };

    // Create the summary log and set parent_id on children
    let ls = LogsState::get_mut(state);
    let summary_id = format!("L{}", ls.next_log_id);
    ls.next_log_id = ls.next_log_id.saturating_add(1);
    let summary = LogEntry {
        id: summary_id.clone(),
        timestamp_ms: max_timestamp,
        content,
        parent_id: None,
        children_ids: log_ids.clone(),
    };
    ls.logs.push(summary);

    // Set parent_id on all children
    for id in &log_ids {
        if let Some(log) = ls.logs.iter_mut().find(|l| l.id == *id) {
            log.parent_id = Some(summary_id.clone());
        }
    }

    touch_logs_panel(state);

    ToolResult::new(tool.id.clone(), format!("Created summary {} with {} children", summary_id, log_ids.len()), false)
}

/// Execute `log_toggle`: expand or collapse a log summary's children.
pub(crate) fn execute_log_toggle(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => {
            return ToolResult::new(tool.id.clone(), "Missing required 'id' parameter".to_string(), true);
        }
    };

    let action = match tool.input.get("action").and_then(|v| v.as_str()) {
        Some(a) if a == "expand" || a == "collapse" => a.to_string(),
        _ => {
            return ToolResult::new(
                tool.id.clone(),
                "Missing or invalid 'action' parameter (must be 'expand' or 'collapse')".to_string(),
                true,
            );
        }
    };

    // Validate: log exists and is a summary (has children)
    {
        let logs = &LogsState::get(state).logs;
        match logs.iter().find(|l| l.id == id) {
            None => {
                return ToolResult::new(tool.id.clone(), format!("Log '{id}' not found"), true);
            }
            Some(log) => {
                if log.children_ids.is_empty() {
                    return ToolResult::new(
                        tool.id.clone(),
                        format!("Log '{id}' has no children — can only toggle summaries"),
                        true,
                    );
                }
            }
        }
    }

    let ls = LogsState::get_mut(state);
    if action == "expand" {
        if !ls.open_log_ids.contains(&id) {
            ls.open_log_ids.push(id.clone());
        }
    } else {
        ls.open_log_ids.retain(|i| i != &id);
    }

    touch_logs_panel(state);

    ToolResult::new(
        tool.id.clone(),
        format!("{} {}", if action == "expand" { "Expanded" } else { "Collapsed" }, id),
        false,
    )
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
        return ToolResult::new(tool.id.clone(), "Cannot close conversation history without at least one log entry. Provide 'logs' with meaningful entries to preserve context before closing.".to_string(), true);
    }

    let mut output_parts = Vec::new();

    // 4. Create log entries (using panel's last message timestamp)
    if let Some(logs_array) = logs_array {
        let mut log_count: usize = 0;
        for log_obj in logs_array {
            if let Some(content) = log_obj.get("content").and_then(|v| v.as_str())
                && !content.is_empty()
            {
                if last_msg_timestamp > 0 {
                    push_log_with_timestamp(state, content.to_string(), last_msg_timestamp);
                } else {
                    push_log(state, content.to_string());
                }
                log_count = log_count.saturating_add(1);
            }
        }
        if log_count > 0 {
            output_parts.push(format!("Created {log_count} log(s)"));
            touch_logs_panel(state);
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
                            "Memory content too long for tl_dr: ~{tokens} tokens (max {MEMORY_TLDR_MAX_TOKENS}). Keep it short."
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
