pub mod panel;
pub mod types;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::constants::{LOGS_CHUNK_SIZE, LOGS_DIR, MEMORY_TLDR_MAX_TOKENS, STORE_DIR};
use crate::core::panels::Panel;
use crate::modules::Module;
use crate::persistence::writer::WriteOp;
use crate::state::{ContextType, State, estimate_tokens};
use crate::tool_defs::{ParamType, ToolCategory, ToolDefinition, ToolParam};
use crate::tools::{ToolResult, ToolUse};

use types::LogEntry;

/// Directory for chunked log files
fn logs_dir() -> PathBuf {
    PathBuf::from(STORE_DIR).join(LOGS_DIR)
}

/// Get chunk index for a log ID number
fn chunk_index(log_id_num: usize) -> usize {
    log_id_num / LOGS_CHUNK_SIZE
}

/// Build WriteOps for chunked log persistence (CPU only — no I/O).
/// Called from save_module_data to integrate with the PersistenceWriter batch system.
pub fn build_log_write_ops(logs: &[LogEntry], next_log_id: usize) -> Vec<WriteOp> {
    let dir = logs_dir();
    let mut ops = Vec::new();

    // Group logs by chunk
    let mut chunks: HashMap<usize, Vec<&LogEntry>> = HashMap::new();
    for log in logs {
        if let Some(num) = log.id.strip_prefix('L').and_then(|n| n.parse::<usize>().ok()) {
            chunks.entry(chunk_index(num)).or_default().push(log);
        }
    }

    // Build WriteOp for each chunk
    for (idx, chunk_logs) in &chunks {
        let path = dir.join(format!("chunk_{}.json", idx));
        if let Ok(json) = serde_json::to_string_pretty(chunk_logs) {
            ops.push(WriteOp { path, content: json.into_bytes() });
        }
    }

    // Build WriteOp for next_id.json
    let next_id_path = dir.join("next_id.json");
    let json = serde_json::json!({ "next_log_id": next_log_id });
    if let Ok(s) = serde_json::to_string_pretty(&json) {
        ops.push(WriteOp { path: next_id_path, content: s.into_bytes() });
    }

    ops
}

/// Load all logs from chunked JSON files in .context-pilot/logs/
fn load_logs_chunked() -> (Vec<LogEntry>, usize) {
    let dir = logs_dir();
    let mut all_logs: Vec<LogEntry> = Vec::new();
    let mut next_log_id: usize = 1;

    // Load next_id.json
    let next_id_path = dir.join("next_id.json");
    if let Ok(content) = fs::read_to_string(&next_id_path)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&content)
        && let Some(v) = val.get("next_log_id").and_then(|v| v.as_u64())
    {
        next_log_id = v as usize;
    }

    // Load all chunk files
    if let Ok(entries) = fs::read_dir(&dir) {
        let mut chunk_files: Vec<(usize, PathBuf)> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let path = e.path();
                let stem = path.file_stem()?.to_str()?;
                let idx = stem.strip_prefix("chunk_")?.parse::<usize>().ok()?;
                Some((idx, path))
            })
            .collect();
        chunk_files.sort_by_key(|(idx, _)| *idx);

        for (_, path) in chunk_files {
            if let Ok(content) = fs::read_to_string(&path)
                && let Ok(logs) = serde_json::from_str::<Vec<LogEntry>>(&content)
            {
                all_logs.extend(logs);
            }
        }
    }

    // Sort by ID number for consistent ordering
    all_logs.sort_by_key(|l| l.id.strip_prefix('L').and_then(|n| n.parse::<usize>().ok()).unwrap_or(0));

    (all_logs, next_log_id)
}

pub struct LogsModule;

impl Module for LogsModule {
    fn id(&self) -> &'static str {
        "logs"
    }
    fn name(&self) -> &'static str {
        "Logs"
    }
    fn description(&self) -> &'static str {
        "Timestamped log entries and conversation history management"
    }
    fn is_core(&self) -> bool {
        false
    }
    fn is_global(&self) -> bool {
        true
    }
    fn dependencies(&self) -> &[&'static str] {
        &["core"]
    }

    fn save_module_data(&self, _state: &State) -> serde_json::Value {
        // Logs are saved via build_log_write_ops() integrated into the WriteBatch,
        // not through the module data JSON. See persistence/mod.rs build_save_batch().
        serde_json::Value::Null
    }

    fn load_module_data(&self, _data: &serde_json::Value, state: &mut State) {
        // Load logs from chunked files on disk
        let (logs, next_log_id) = load_logs_chunked();
        if !logs.is_empty() || next_log_id > 1 {
            state.logs = logs;
            state.next_log_id = next_log_id;
        }
    }

    fn save_worker_data(&self, state: &State) -> serde_json::Value {
        serde_json::json!({
            "open_log_ids": state.open_log_ids,
        })
    }

    fn load_worker_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(arr) = data.get("open_log_ids").and_then(|v| v.as_array()) {
            state.open_log_ids = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
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
                id: "log_summarize".to_string(),
                name: "Summarize Logs".to_string(),
                short_desc: "Summarize multiple logs into a parent log".to_string(),
                description: "Summarizes multiple top-level log entries into a single parent summary log. The original logs become children hidden under the summary. Only top-level logs (no parent) can be summarized. Minimum 10 entries required.".to_string(),
                params: vec![
                    ToolParam::new("log_ids", ParamType::Array(Box::new(ParamType::String)))
                        .desc("Array of log IDs to summarize (e.g., ['L27', 'L28', 'L29']). Minimum 10 entries. All must be top-level (no parent).")
                        .required(),
                    ToolParam::new("content", ParamType::String)
                        .desc("Summary text for the new parent log entry")
                        .required(),
                ],
                enabled: true,
                category: ToolCategory::Context,
            },

            ToolDefinition {
                id: "log_toggle".to_string(),
                name: "Toggle Log Summary".to_string(),
                short_desc: "Expand or collapse a log summary".to_string(),
                description: "Expands or collapses a log summary to show or hide its children. Can only toggle logs that have children (are summaries).".to_string(),
                params: vec![
                    ToolParam::new("id", ParamType::String)
                        .desc("Log ID to toggle (e.g., 'L42')")
                        .required(),
                    ToolParam::new("action", ParamType::String)
                        .desc("Action to perform")
                        .enum_vals(&["expand", "collapse"])
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
            "log_summarize" => Some(execute_log_summarize(tool, state)),
            "log_toggle" => Some(execute_log_toggle(tool, state)),
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

/// Helper: touch logs panel to update last_refresh_ms and recalculate token count.
/// The Logs panel renders live from state.logs (no background cache), so
/// cache_deprecated is meaningless for it. Instead we bump refresh time
/// for correct LLM panel ordering and update token_count for sidebar display.
fn touch_logs_panel(state: &mut State) {
    let content = panel::LogsPanel::format_logs_tree(state);
    let token_count = estimate_tokens(&content);
    let now = crate::core::panels::now_ms();
    for ctx in &mut state.context {
        if ctx.context_type == ContextType::Logs {
            ctx.token_count = token_count;
            ctx.last_refresh_ms = now;
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
        if let Some(content) = entry_obj.get("content").and_then(|v| v.as_str())
            && !content.is_empty()
        {
            push_log(state, content.to_string());
            count += 1;
        }
    }

    if count > 0 {
        touch_logs_panel(state);
    }

    ToolResult { tool_use_id: tool.id.clone(), content: format!("Created {} log(s)", count), is_error: false }
}

fn execute_log_summarize(tool: &ToolUse, state: &mut State) -> ToolResult {
    // Parse log_ids
    let log_ids: Vec<String> = match tool.input.get("log_ids").and_then(|v| v.as_array()) {
        Some(arr) => arr.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required 'log_ids' array".to_string(),
                is_error: true,
            };
        }
    };

    // Parse content
    let content = match tool.input.get("content").and_then(|v| v.as_str()) {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required 'content' parameter".to_string(),
                is_error: true,
            };
        }
    };

    // Guardrail: minimum 10 entries
    if log_ids.len() < 10 {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Must summarize at least 10 logs, got {}", log_ids.len()),
            is_error: true,
        };
    }

    // Validate: all IDs exist and are top-level
    for id in &log_ids {
        match state.logs.iter().find(|l| l.id == *id) {
            None => {
                return ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Log '{}' not found", id),
                    is_error: true,
                };
            }
            Some(log) => {
                if log.parent_id.is_some() {
                    return ToolResult {
                        tool_use_id: tool.id.clone(),
                        content: format!("Log '{}' already has a parent — only top-level logs can be summarized", id),
                        is_error: true,
                    };
                }
            }
        }
    }

    // Compute timestamp = max of children timestamps
    let max_timestamp = log_ids
        .iter()
        .filter_map(|id| state.logs.iter().find(|l| l.id == *id))
        .map(|l| l.timestamp_ms)
        .max()
        .unwrap_or(0);

    // Create the summary log
    let summary_id = format!("L{}", state.next_log_id);
    state.next_log_id += 1;
    let summary = LogEntry {
        id: summary_id.clone(),
        timestamp_ms: max_timestamp,
        content,
        parent_id: None,
        children_ids: log_ids.clone(),
    };
    state.logs.push(summary);

    // Set parent_id on all children
    for id in &log_ids {
        if let Some(log) = state.logs.iter_mut().find(|l| l.id == *id) {
            log.parent_id = Some(summary_id.clone());
        }
    }

    touch_logs_panel(state);

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Created summary {} with {} children", summary_id, log_ids.len()),
        is_error: false,
    }
}

fn execute_log_toggle(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required 'id' parameter".to_string(),
                is_error: true,
            };
        }
    };

    let action = match tool.input.get("action").and_then(|v| v.as_str()) {
        Some(a) if a == "expand" || a == "collapse" => a.to_string(),
        _ => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing or invalid 'action' parameter (must be 'expand' or 'collapse')".to_string(),
                is_error: true,
            };
        }
    };

    // Validate: log exists and is a summary (has children)
    match state.logs.iter().find(|l| l.id == id) {
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Log '{}' not found", id),
                is_error: true,
            };
        }
        Some(log) => {
            if log.children_ids.is_empty() {
                return ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Log '{}' has no children — can only toggle summaries", id),
                    is_error: true,
                };
            }
        }
    }

    if action == "expand" {
        if !state.open_log_ids.contains(&id) {
            state.open_log_ids.push(id.clone());
        }
    } else {
        state.open_log_ids.retain(|i| i != &id);
    }

    touch_logs_panel(state);

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("{} {}", if action == "expand" { "Expanded" } else { "Collapsed" }, id),
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
                    content: format!(
                        "Panel '{}' is not a conversation history panel (type: {:?})",
                        panel_id, state.context[idx].context_type
                    ),
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
    let has_logs = logs_array.is_some_and(|arr| {
        arr.iter().any(|e| e.get("content").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty()))
    });

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
            if let Some(content) = log_obj.get("content").and_then(|v| v.as_str())
                && !content.is_empty()
            {
                if last_msg_timestamp > 0 {
                    push_log_with_timestamp(state, content.to_string(), last_msg_timestamp);
                } else {
                    push_log(state, content.to_string());
                }
                log_count += 1;
            }
        }
        if log_count > 0 {
            output_parts.push(format!("Created {} log(s)", log_count));
            touch_logs_panel(state);
        }
    }

    // 5. Create memory items
    if let Some(memories_array) = tool.input.get("memories").and_then(|v| v.as_array()) {
        let mut mem_count = 0;
        for mem_obj in memories_array {
            if let Some(content) = mem_obj.get("content").and_then(|v| v.as_str())
                && !content.is_empty()
            {
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

                let importance = mem_obj.get("importance").and_then(|v| v.as_str()).unwrap_or("medium");

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
        if mem_count > 0 {
            output_parts.push(format!("Created {} memory(ies)", mem_count));
            // Deprecate the memory panel cache
            crate::core::panels::mark_panels_dirty(state, ContextType::Memory);
        }
    }

    // 6. Close the conversation history panel
    let panel_name = state.context.iter().find(|c| c.id == panel_id).map(|c| c.name.clone()).unwrap_or_default();
    state.context.retain(|c| c.id != panel_id);
    output_parts.push(format!("Closed {} ({})", panel_id, panel_name));

    ToolResult { tool_use_id: tool.id.clone(), content: output_parts.join("\n"), is_error: false }
}
