mod panel;
mod tools;
pub mod types;

pub use types::{LogEntry, LogsState};

/// Logs subdirectory (chunked JSON files, global across workers)
pub const LOGS_DIR: &str = "logs";

/// Number of log entries per chunk file
pub const LOGS_CHUNK_SIZE: usize = 1000;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use cp_base::config::constants::STORE_DIR;
use cp_base::modules::{Module, ToolVisualizer};
use cp_base::panels::Panel;
use cp_base::state::{ContextType, State};
use cp_base::tools::{ParamType, PreFlightResult, ToolDefinition, ToolParam, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> = std::sync::LazyLock::new(|| {
    serde_yaml::from_str(include_str!("../../../yamls/tools/logs.yaml")).expect("Failed to parse logs tool YAML")
});

/// Directory for chunked log files
fn logs_dir() -> PathBuf {
    PathBuf::from(STORE_DIR).join(LOGS_DIR)
}

/// Get chunk index for a log ID number
fn chunk_index(log_id_num: usize) -> usize {
    log_id_num / LOGS_CHUNK_SIZE
}

/// Build write operations for chunked log persistence (CPU only — no I/O).
/// Called from save_module_data to integrate with the PersistenceWriter batch system.
/// Returns Vec<(path, content)> tuples that the binary converts to WriteOps.
pub fn build_log_write_ops(logs: &[LogEntry], next_log_id: usize) -> Vec<(PathBuf, Vec<u8>)> {
    let dir = logs_dir();
    let mut ops = Vec::new();

    // Group logs by chunk
    let mut chunks: HashMap<usize, Vec<&LogEntry>> = HashMap::new();
    for log in logs {
        if let Some(num) = log.id.strip_prefix('L').and_then(|n| n.parse::<usize>().ok()) {
            chunks.entry(chunk_index(num)).or_default().push(log);
        }
    }

    // Build write op for each chunk
    for (idx, chunk_logs) in &chunks {
        let path = dir.join(format!("chunk_{}.json", idx));
        if let Ok(json) = serde_json::to_string_pretty(chunk_logs) {
            ops.push((path, json.into_bytes()));
        }
    }

    // Build write op for next_id.json
    let next_id_path = dir.join("next_id.json");
    let json = serde_json::json!({ "next_log_id": next_log_id });
    if let Ok(s) = serde_json::to_string_pretty(&json) {
        ops.push((next_id_path, s.into_bytes()));
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

    fn init_state(&self, state: &mut State) {
        state.set_ext(LogsState::new());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(LogsState::new());
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
            let ls = LogsState::get_mut(state);
            ls.logs = logs;
            ls.next_log_id = next_log_id;
        }
    }

    fn save_worker_data(&self, state: &State) -> serde_json::Value {
        serde_json::json!({
            "open_log_ids": LogsState::get(state).open_log_ids,
        })
    }

    fn load_worker_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(arr) = data.get("open_log_ids").and_then(|v| v.as_array()) {
            LogsState::get_mut(state).open_log_ids = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("log_create", t)
                .short_desc("Create timestamped log entries")
                .category("Context")
                .reverie_allowed(true)
                .param_array(
                    "entries",
                    ParamType::Object(vec![
                        ToolParam::new("content", ParamType::String).desc("Short, atomic log entry").required(),
                    ]),
                    true,
                )
                .build(),
            ToolDefinition::from_yaml("log_summarize", t)
                .short_desc("Summarize multiple logs into a parent log")
                .category("Context")
                .reverie_allowed(true)
                .param_array("log_ids", ParamType::String, true)
                .param("content", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("log_toggle", t)
                .short_desc("Expand or collapse a log summary")
                .category("Context")
                .param("id", ParamType::String, true)
                .param_enum("action", &["expand", "collapse"], true)
                .build(),
            ToolDefinition::from_yaml("Close_conversation_history", t)
                .short_desc("Close a conversation history panel with logs/memories")
                .category("Context")
                .reverie_allowed(true)
                .param("id", ParamType::String, true)
                .param_array(
                    "logs",
                    ParamType::Object(vec![
                        ToolParam::new("content", ParamType::String)
                            .desc("Short, atomic log entry to remember")
                            .required(),
                    ]),
                    false,
                )
                .param_array(
                    "memories",
                    ParamType::Object(vec![
                        ToolParam::new("content", ParamType::String).desc("Memory content").required(),
                        ToolParam::new("importance", ParamType::String)
                            .desc("Importance level")
                            .enum_vals(&["low", "medium", "high", "critical"]),
                    ]),
                    false,
                )
                .build(),
        ]
    }

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<PreFlightResult> {
        match tool.name.as_str() {
            "log_summarize" => {
                let mut pf = PreFlightResult::new();
                if let Some(ids) = tool.input.get("log_ids").and_then(|v| v.as_array()) {
                    let logs = &LogsState::get(state).logs;
                    for id_val in ids {
                        if let Some(id) = id_val.as_str()
                            && !logs.iter().any(|l| l.id == id)
                        {
                            pf.errors.push(format!("Log '{}' not found", id));
                        }
                    }
                }
                Some(pf)
            }
            "log_toggle" => {
                let mut pf = PreFlightResult::new();
                if let Some(id) = tool.input.get("id").and_then(|v| v.as_str()) {
                    let logs = &LogsState::get(state).logs;
                    match logs.iter().find(|l| l.id == id) {
                        None => pf.errors.push(format!("Log '{}' not found", id)),
                        Some(log) if log.children_ids.is_empty() => {
                            pf.errors.push(format!("Log '{}' has no children — can only toggle summaries", id));
                        }
                        _ => {}
                    }
                }
                Some(pf)
            }
            "Close_conversation_history" => {
                let mut pf = PreFlightResult::new();
                // Queue must be active for destructive history panel operations
                if !cp_mod_queue::QueueState::get(state).active {
                    pf.errors.push(
                        "Cannot close conversation history without an active queue. \
                         Activate the queue first (Queue_activate), then queue this call. \
                         Closing history panels causes irreversible context loss."
                            .to_string(),
                    );
                }
                if let Some(id) = tool.input.get("id").and_then(|v| v.as_str()) {
                    match state.context.iter().find(|c| c.id == id) {
                        None => pf.errors.push(format!("Panel '{}' not found", id)),
                        Some(ctx) if ctx.context_type != ContextType::CONVERSATION_HISTORY => {
                            pf.errors.push(format!(
                                "Panel '{}' is not a conversation history panel — use Close_panel instead",
                                id
                            ));
                        }
                        _ => {}
                    }
                }
                Some(pf)
            }
            _ => None,
        }
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "log_create" => Some(tools::execute_log_create(tool, state)),
            "log_summarize" => Some(tools::execute_log_summarize(tool, state)),
            "log_toggle" => Some(tools::execute_log_toggle(tool, state)),
            "Close_conversation_history" => Some(tools::execute_close_conversation_history(tool, state)),
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![
            ("log_create", visualize_logs_output as ToolVisualizer),
            ("log_summarize", visualize_logs_output as ToolVisualizer),
            ("log_toggle", visualize_logs_output as ToolVisualizer),
            ("Close_conversation_history", visualize_logs_output as ToolVisualizer),
        ]
    }

    fn create_panel(&self, context_type: &ContextType) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            ContextType::LOGS => Some(Box::new(panel::LogsPanel)),
            _ => None,
        }
    }

    fn fixed_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::new(ContextType::LOGS)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> {
        vec![(ContextType::new(ContextType::LOGS), "Logs", true)]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::ContextTypeMeta> {
        vec![cp_base::state::ContextTypeMeta {
            context_type: "logs",
            icon_id: "memory",
            is_fixed: true,
            needs_cache: false,
            fixed_order: Some(6),
            display_name: "logs",
            short_name: "logs",
            needs_async_wait: false,
        }]
    }
}

/// Visualizer for logs tool results.
/// Highlights timestamps, log entry content, and summary operations.
fn visualize_logs_output(content: &str, width: usize) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::prelude::*;

    let success_color = Color::Rgb(80, 250, 123);
    let info_color = Color::Rgb(139, 233, 253);
    let warning_color = Color::Rgb(241, 250, 140);
    let error_color = Color::Rgb(255, 85, 85);

    let mut lines = Vec::new();

    for line in content.lines() {
        if line.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        let style = if line.starts_with("Error:") {
            Style::default().fg(error_color)
        } else if line.starts_with("Created") {
            Style::default().fg(success_color)
        } else if line.contains("summary") || line.contains("Summary") {
            Style::default().fg(info_color)
        } else if line.contains("Expanded") || line.contains("Collapsed") {
            Style::default().fg(warning_color)
        } else if line.starts_with("Closed") {
            Style::default().fg(success_color)
        } else if line.starts_with("L") && line.chars().nth(1).is_some_and(|c| c.is_ascii_digit()) {
            // Log IDs like L1, L2
            Style::default().fg(info_color)
        } else {
            Style::default()
        };

        let display = if line.len() > width {
            format!("{}...", &line[..line.floor_char_boundary(width.saturating_sub(3))])
        } else {
            line.to_string()
        };
        lines.push(Line::from(Span::styled(display, style)));
    }

    lines
}
