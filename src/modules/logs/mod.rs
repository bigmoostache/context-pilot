pub mod panel;
pub mod types;

use crate::core::panels::Panel;
use crate::modules::Module;
use crate::state::{ContextType, State, estimate_tokens};
use crate::tool_defs::{ParamType, ToolCategory, ToolDefinition, ToolParam};
use crate::tools::{ToolResult, ToolUse};
use crate::constants::MEMORY_TLDR_MAX_TOKENS;

use types::LogEntry;

pub struct LogsModule;

impl Module for LogsModule {
    fn id(&self) -> &'static str { "logs" }
    fn name(&self) -> &'static str { "Logs" }
    fn description(&self) -> &'static str { "Timestamped log entries and conversation history management" }
    fn is_core(&self) -> bool { false }
    fn is_global(&self) -> bool { false }
    fn dependencies(&self) -> &[&'static str] { &["core"] }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        serde_json::json!({
            "logs": state.logs,
            "next_log_id": state.next_log_id,
        })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(arr) = data.get("logs") {
            if let Ok(logs) = serde_json::from_value::<Vec<LogEntry>>(arr.clone()) {
                state.logs = logs;
            }
        }
        if let Some(v) = data.get("next_log_id").and_then(|v| v.as_u64()) {
            state.next_log_id = v as usize;
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                id: "log_create".to_string(),
                name: "Create Logs".to_string(),
                short_desc: "Create timestamped log entries".to_string(),
                description: "Creates timestamped log entries for recording decisions, actions, or notable events during the conversation.".to_string(),
                params: vec![
                    ToolParam::new("entries", ParamType::Array(Box::new(ParamType::Object(vec![
                        ToolParam::new("content", ParamType::String)
                            .desc("Short, atomic log entry")
                            .required(),
                    ]))))
                        .desc("Array of log entries to create (timestamped automatically)")
                        .required(),
                ],
                enabled: true,
                category: ToolCategory::Context,
            },

            ToolDefinition {
                id: "close_conversation_history".to_string(),
                name: "Close Conversation History".to_string(),
                short_desc: "Close a conversation history panel with logs/memories".to_string(),
                description: "Closes a conversation history panel. Before closing, creates log entries and memory items to preserve important information. Everything in the history panel will be lost after closing.\n\nIMPORTANT: Before calling this tool, carefully review ALL human messages in the panel. Extract and preserve:\n- **Logs**: Short, atomic entries for mid-conversation context — decisions made, actions taken, bugs found, user preferences expressed, task context. Logs are medium-lifecycle: they maintain conversation flow across reloads.\n- **Memories**: Longer-lived knowledge that matters beyond this session — architecture decisions, user preferences, project conventions, important discoveries. Memories persist across conversations.\n\nEvery substantive piece of information from human messages should be captured in either a log or a memory. When in doubt, preserve it. Log timestamps are set to the panel's last message time, not the current time.".to_string(),
                params: vec![
                    ToolParam::new("id", ParamType::String)
                        .desc("ID of the conversation history panel to close (e.g., 'P12')")
                        .required(),
                    ToolParam::new("logs", ParamType::Array(Box::new(ParamType::Object(vec![
                        ToolParam::new("content", ParamType::String)
                            .desc("Short, atomic log entry to remember")
                            .required(),
                    ]))))
                        .desc("Log entries to create (timestamped automatically). Should be short, atomic things to remember from the conversation."),
                    ToolParam::new("memories", ParamType::Array(Box::new(ParamType::Object(vec![
                        ToolParam::new("content", ParamType::String)
                            .desc("Memory content")
                            .required(),
                        ToolParam::new("importance", ParamType::String)
                            .desc("Importance level")
                            .enum_vals(&["low", "medium", "high", "critical"]),
                    ]))))
                        .desc("Memory items to create (persistent across conversations)"),
                ],
                enabled: true,
                category: ToolCategory::Context,
            },
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "log_create" => Some(execute_log_create(tool, state)),

            "close_conversation_history" => Some(execute_close_conversation_history(tool, state)),
            _ => None,
        }
    }

    fn create_panel(&self, context_type: ContextType) -> Option<Box<dyn Panel>> {
        match context_type {
            ContextType::Logs => Some(Box::new(panel::LogsPanel)),
            _ => None,
        }
    }

    fn fixed_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::Logs]
    }

    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> {
        vec![(ContextType::Logs, "Logs", true)]
    }
}

/// Helper: allocate a log ID and push a log entry (timestamped now)
fn push_log(state: &mut State, content: String) {
    let id = format!("L{}", state.next_log_id);
    state.next_log_id += 1;
    state.logs.push(LogEntry::new(id, content));
}

/// Helper: allocate a log ID and push a log entry with an explicit timestamp
fn push_log_with_timestamp(state: &mut State, content: String, timestamp_ms: u64) {
    let id = format!("L{}", state.next_log_id);
    state.next_log_id += 1;
    state.logs.push(LogEntry::with_timestamp(id, content, timestamp_ms));
}

/// Helper: mark logs panel cache as deprecated
fn deprecate_logs_cache(state: &mut State) {
    for ctx in &mut state.context {
        if ctx.context_type == ContextType::Logs {
            ctx.cache_deprecated = true;
        }
    }
}

fn execute_log_create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let entries = match tool.input.get("entries").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required 'entries' array".to_string(),
                is_error: true,
            };
        }
    };

    if entries.is_empty() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "Empty 'entries' array".to_string(),
            is_error: true,
        };
    }

    let mut count = 0;
    for entry_obj in entries {
        if let Some(content) = entry_obj.get("content").and_then(|v| v.as_str()) {
            if !content.is_empty() {
                push_log(state, content.to_string());
                count += 1;
            }
        }
    }

    if count > 0 {
        deprecate_logs_cache(state);
    }

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Created {} log(s)", count),
        is_error: false,
    }
}



fn execute_close_conversation_history(tool: &ToolUse, state: &mut State) -> ToolResult {
    // 1. Validate the panel ID
    let panel_id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required 'id' parameter".to_string(),
                is_error: true,
            };
        }
    };

    // Find the panel and verify it's a ConversationHistory
    let panel_idx = state.context.iter().position(|c| c.id == panel_id);
    match panel_idx {
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Panel '{}' not found", panel_id),
                is_error: true,
            };
        }
        Some(idx) => {
            if state.context[idx].context_type != ContextType::ConversationHistory {
                return ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Panel '{}' is not a conversation history panel (type: {:?})", panel_id, state.context[idx].context_type),
                    is_error: true,
                };
            }
        }
    }

    // 2. Extract the last message timestamp from the panel
    let panel_idx = panel_idx.unwrap();
    let last_msg_timestamp = state.context[panel_idx]
        .history_messages
        .as_ref()
        .and_then(|msgs| msgs.last())
        .map(|msg| msg.timestamp_ms)
        .unwrap_or(0); // fallback to 0 means LogEntry::new will be used instead

    // 3. Validate that logs are provided (at least one non-empty entry)
    let logs_array = tool.input.get("logs").and_then(|v| v.as_array());
    let has_logs = logs_array
        .map(|arr| arr.iter().any(|e| {
            e.get("content").and_then(|v| v.as_str()).map_or(false, |s| !s.is_empty())
        }))
        .unwrap_or(false);

    if !has_logs {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "Cannot close conversation history without at least one log entry. Provide 'logs' with meaningful entries to preserve context before closing.".to_string(),
            is_error: true,
        };
    }

    let mut output_parts = Vec::new();

    // 4. Create log entries (using panel's last message timestamp)
    if let Some(logs_array) = logs_array {
        let mut log_count = 0;
        for log_obj in logs_array {
            if let Some(content) = log_obj.get("content").and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    if last_msg_timestamp > 0 {
                        push_log_with_timestamp(state, content.to_string(), last_msg_timestamp);
                    } else {
                        push_log(state, content.to_string());
                    }
                    log_count += 1;
                }
            }
        }
        if log_count > 0 {
            output_parts.push(format!("Created {} log(s)", log_count));
            deprecate_logs_cache(state);
        }
    }

    // 5. Create memory items
    if let Some(memories_array) = tool.input.get("memories").and_then(|v| v.as_array()) {
        let mut mem_count = 0;
        for mem_obj in memories_array {
            if let Some(content) = mem_obj.get("content").and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    // Validate tl_dr length
                    let tokens = estimate_tokens(content);
                    if tokens > MEMORY_TLDR_MAX_TOKENS {
                        return ToolResult {
                            tool_use_id: tool.id.clone(),
                            content: format!(
                                "Memory content too long for tl_dr: ~{} tokens (max {}). Keep it short.",
                                tokens, MEMORY_TLDR_MAX_TOKENS
                            ),
                            is_error: true,
                        };
                    }

                    let importance = mem_obj
                        .get("importance")
                        .and_then(|v| v.as_str())
                        .unwrap_or("medium");

                    let importance_level = match importance {
                        "low" => crate::state::MemoryImportance::Low,
                        "high" => crate::state::MemoryImportance::High,
                        "critical" => crate::state::MemoryImportance::Critical,
                        _ => crate::state::MemoryImportance::Medium,
                    };

                    let id = format!("M{}", state.next_memory_id);
                    state.next_memory_id += 1;
                    state.memories.push(crate::state::MemoryItem {
                        id,
                        tl_dr: content.to_string(),
                        contents: String::new(),
                        importance: importance_level,
                        labels: vec![],
                    });
                    mem_count += 1;
                }
            }
        }
        if mem_count > 0 {
            output_parts.push(format!("Created {} memory(ies)", mem_count));
            // Deprecate the memory panel cache
            for ctx in &mut state.context {
                if ctx.context_type == ContextType::Memory {
                    ctx.cache_deprecated = true;
                }
            }
        }
    }

    // 6. Close the conversation history panel
    let panel_name = state.context.iter()
        .find(|c| c.id == panel_id)
        .map(|c| c.name.clone())
        .unwrap_or_default();
    state.context.retain(|c| c.id != panel_id);
    output_parts.push(format!("Closed {} ({})", panel_id, panel_name));

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: output_parts.join("\n"),
        is_error: false,
    }
}
