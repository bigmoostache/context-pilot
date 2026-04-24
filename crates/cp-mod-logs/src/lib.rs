//! Logs module — timestamped entries, summarization, and conversation history.
//!
//! Four tools: `log_create`, `log_summarize` (collapse multiple into parent),
//! `log_toggle` (expand/collapse summaries), `Close_conversation_history`
//! (archive a history panel with log/memory extraction). Logs are stored
//! globally in chunked JSON files under `.context-pilot/logs/`.

/// Logs panel: tree-structured display of log entries with summaries.
mod panel;
/// Tool implementations: create, summarize, toggle, close conversation history.
mod tools;
/// Log state types: `LogEntry`, `LogsState`.
pub mod types;

use types::{LogEntry, LogsState};

use cp_base::cast::Safe as _;

/// Logs subdirectory (chunked JSON files, global across workers)
pub const LOGS_DIR: &str = "logs";

/// Number of log entries per chunk file
pub const LOGS_CHUNK_SIZE: usize = 1000;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use cp_base::config::constants;
use cp_base::modules::{Module, ToolVisualizer};
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ParamType, ToolDefinition, ToolParam, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

/// Lazily parsed tool texts from the logs YAML definition file.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/logs.yaml")));

/// Directory for chunked log files
fn logs_dir() -> PathBuf {
    PathBuf::from(constants::STORE_DIR).join(LOGS_DIR)
}

/// Get chunk index for a log ID number
const fn chunk_index(log_id_num: usize) -> usize {
    cp_base::panels::time_arith::div_const::<LOGS_CHUNK_SIZE>(log_id_num)
}

/// Build write operations for chunked log persistence (CPU only — no I/O).
///
/// Called from `save_module_data` to integrate with the `PersistenceWriter` batch system.
/// Returns Vec<(path, content)> tuples that the binary converts to `WriteOps`.
#[must_use]
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

    // Build write op for each chunk (sorted by index for deterministic output)
    let mut sorted_chunk_keys: Vec<_> = chunks.keys().copied().collect();
    sorted_chunk_keys.sort_unstable();
    for idx in sorted_chunk_keys {
        if let Some(chunk_logs) = chunks.get(&idx) {
            let path = dir.join(format!("chunk_{idx}.json"));
            if let Ok(json) = serde_json::to_string_pretty(chunk_logs) {
                ops.push((path, json.into_bytes()));
            }
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
        && let Some(v) = val.get("next_log_id").and_then(serde_json::Value::as_u64)
    {
        next_log_id = v.to_usize();
    }

    // Load all chunk files
    if let Ok(entries) = fs::read_dir(&dir) {
        let mut chunk_files: Vec<(usize, PathBuf)> = entries
            .filter_map(Result::ok)
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

/// Logs module: timestamped entries, summarization, and conversation history management.
#[derive(Debug, Clone, Copy)]
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

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<Verdict> {
        match tool.name.as_str() {
            "log_create" => {
                let mut pf = Verdict::new();
                pf.activate_queue = true;
                Some(pf)
            }
            "log_summarize" => {
                let mut pf = Verdict::new();
                pf.activate_queue = true;
                if let Some(ids) = tool.input.get("log_ids").and_then(|v| v.as_array()) {
                    let logs = &LogsState::get(state).logs;
                    for id_val in ids {
                        if let Some(id) = id_val.as_str()
                            && !logs.iter().any(|l| l.id == id)
                        {
                            pf.errors.push(format!("Log '{id}' not found"));
                        }
                    }
                }
                Some(pf)
            }
            "log_toggle" => {
                let mut pf = Verdict::new();
                pf.activate_queue = true;
                if let Some(id) = tool.input.get("id").and_then(|v| v.as_str()) {
                    let logs = &LogsState::get(state).logs;
                    match logs.iter().find(|l| l.id == id) {
                        None => pf.errors.push(format!("Log '{id}' not found")),
                        Some(log) if log.children_ids.is_empty() => {
                            pf.errors.push(format!("Log '{id}' has no children — can only toggle summaries"));
                        }
                        _ => {}
                    }
                }
                Some(pf)
            }
            "Close_conversation_history" => {
                let mut pf = Verdict::new();
                // Auto-activate queue — closing history panels is destructive
                pf.activate_queue = true;
                if let Some(id) = tool.input.get("id").and_then(|v| v.as_str()) {
                    match state.context.iter().find(|c| c.id == id) {
                        None => pf.errors.push(format!("Panel '{id}' not found")),
                        Some(ctx) if ctx.context_type.as_str() != Kind::CONVERSATION_HISTORY => {
                            pf.errors.push(format!(
                                "Panel '{id}' is not a conversation history panel — use Close_panel instead"
                            ));
                        }
                        _ => {}
                    }
                }
                // Validate memory content lengths before execution
                if let Some(memories) = tool.input.get("memories").and_then(|v| v.as_array()) {
                    let max_tokens = cp_mod_memory::MEMORY_TLDR_MAX_TOKENS;
                    for (i, mem) in memories.iter().enumerate() {
                        if let Some(content) = mem.get("content").and_then(|v| v.as_str()) {
                            let approx_tokens = cp_base::panels::time_arith::div_const::<3>(
                                content.split_whitespace().count().saturating_mul(4),
                            );
                            if approx_tokens > max_tokens {
                                pf.errors.push(format!(
                                    "Memory #{} content too long: ~{} tokens (max {}). Shorten it.",
                                    i.saturating_add(1),
                                    approx_tokens,
                                    max_tokens,
                                ));
                            }
                        }
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
            ("log_create", visualize_logs_output),
            ("log_summarize", visualize_logs_output),
            ("log_toggle", visualize_logs_output),
            ("Close_conversation_history", visualize_logs_output),
        ]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            Kind::LOGS => Some(Box::new(panel::LogsPanel)),
            _ => None,
        }
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(Kind::LOGS)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![(Kind::new(Kind::LOGS), "Logs", true)]
    }

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![cp_base::state::context::TypeMeta {
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

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![]
    }

    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
        None
    }

    fn context_detail(&self, _ctx: &cp_base::state::context::Entry) -> Option<String> {
        None
    }

    fn overview_context_section(&self, _state: &State) -> Option<String> {
        None
    }

    fn overview_render_sections(&self, _state: &State) -> Vec<(u8, Vec<cp_render::Block>)> {
        vec![]
    }

    fn on_close_context(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _state: &mut State,
    ) -> Option<Result<String, String>> {
        None
    }

    fn watch_paths(&self, _state: &State) -> Vec<cp_base::panels::WatchSpec> {
        vec![]
    }

    fn should_invalidate_on_fs_change(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _changed_path: &str,
        _is_dir_event: bool,
    ) -> bool {
        false
    }

    fn watcher_immediate_refresh(&self) -> bool {
        true
    }

    fn on_user_message(&self, _state: &mut State) {}

    fn on_stream_stop(&self, _state: &mut State) {}

    fn on_tool_progress(&self, _tool_name: &str, _input_so_far: &str, _state: &mut State) {}

    fn on_tool_complete(&self, _tool_name: &str, _state: &mut State) {}
}

/// Visualizer for logs tool results.
/// Highlights timestamps, log entry content, and summary operations.
fn visualize_logs_output(content: &str, width: usize) -> Vec<cp_render::Block> {
    use cp_render::{Block, Semantic, Span};

    content
        .lines()
        .map(|line| {
            if line.is_empty() {
                return Block::empty();
            }
            let semantic = if line.starts_with("Error:") {
                Semantic::Error
            } else if line.starts_with("Created") || line.starts_with("Closed") {
                Semantic::Success
            } else if line.contains("summary") || line.contains("Summary") {
                Semantic::Info
            } else if line.contains("Expanded") || line.contains("Collapsed") {
                Semantic::Warning
            } else if line.starts_with('L') && line.chars().nth(1).is_some_and(|c| c.is_ascii_digit()) {
                Semantic::Info
            } else {
                Semantic::Default
            };
            let display = if line.len() > width {
                format!("{}...", line.get(..line.floor_char_boundary(width.saturating_sub(3))).unwrap_or(""))
            } else {
                line.to_string()
            };
            Block::Line(vec![Span::styled(display, semantic)])
        })
        .collect()
}
