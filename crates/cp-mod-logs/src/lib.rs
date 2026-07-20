//! Logs module — timestamped entries and conversation history.
//!
//! Two tools: `log_create` (with tags + importance), `Close_conversation_history`
//! (archive a history panel with log/memory extraction). Logs are stored
//! globally in chunked JSON files under `.context-pilot/logs/` and indexed
//! by the search module for full-text retrieval.

/// Tool implementations: create, close conversation history.
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

/// Schema version marker file path.
fn schema_marker_path() -> PathBuf {
    logs_dir().join(".schema_v2")
}

/// Delete all `chunk_*.json` and `next_id.json` files in the logs directory.
fn purge_chunk_files(dir: &std::path::Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("chunk_") || n == "next_id.json") {
            let _r = fs::remove_file(&path);
        }
    }
}

/// Perform clean-slate migration to the v2 schema (tags + importance, no summaries).
///
/// If the marker file `.schema_v2` does not exist in the logs directory,
/// all existing chunk files are deleted and the counter is reset.
fn migrate_if_needed() {
    let marker = schema_marker_path();
    if marker.exists() {
        return;
    }

    let dir = logs_dir();
    if dir.is_dir() {
        purge_chunk_files(&dir);
    } else {
        let _r = fs::create_dir_all(&dir);
    }

    // Write reset next_id.json
    let next_id_json = serde_json::json!({ "next_log_id": 1i32 });
    if let Ok(s) = serde_json::to_string_pretty(&next_id_json) {
        let _r = fs::write(dir.join("next_id.json"), s);
    }

    // Write marker
    let _r = fs::write(&marker, "migrated");
    log::info!("Logs: clean-slate migration to v2 schema complete");
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

/// Logs module: timestamped entries and conversation history management.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct LogsModule;

impl Default for LogsModule {
    fn default() -> Self {
        Self::new()
    }
}

impl LogsModule {
    /// Construct the module marker (funnels cross-crate construction of this
    /// `non_exhaustive` unit struct through an associated fn).
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

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
        migrate_if_needed();
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
        migrate_if_needed();

        // Load logs from chunked files on disk
        let (logs, next_log_id) = load_logs_chunked();
        if !logs.is_empty() || next_log_id > 1 {
            let ls = LogsState::get_mut(state);
            ls.logs = logs;
            ls.next_log_id = next_log_id;
        }
    }

    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}

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
                        ToolParam::new("importance", ParamType::String)
                            .desc("Importance level")
                            .enum_vals(&["low", "medium", "high", "critical"]),
                    ]),
                    true,
                )
                .build(),
            ToolDefinition::from_yaml("Close_conversation_history", t)
                .short_desc("Close conversation history panels with logs")
                .category("Context")
                .reverie_allowed(true)
                .param_array(
                    "panels",
                    ParamType::Object(vec![
                        ToolParam::new("panel_id", ParamType::String)
                            .desc("ID of the conversation history panel to close (e.g., 'P12')")
                            .required(),
                        ToolParam::new("logs", ParamType::Array(Box::new(ParamType::String)))
                            .desc("Log entries to create — simple strings, one per entry"),
                    ]),
                    true,
                )
                .build(),
        ]
    }

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<Verdict> {
        match tool.name.as_str() {
            "Close_conversation_history" => {
                let mut pf = Verdict::new();
                // Auto-activate queue — closing history panels is destructive
                pf.activate_queue = true;

                // Panels array: must exist and be non-empty
                let Some(panels) = tool.input.get("panels").and_then(|v| v.as_array()) else {
                    pf.errors.push("Missing required 'panels' array".to_owned());
                    return Some(pf);
                };
                if panels.is_empty() {
                    pf.errors.push("Empty 'panels' array — provide at least one panel to close".to_owned());
                    return Some(pf);
                }

                for (i, panel_obj) in panels.iter().enumerate() {
                    let idx = i.saturating_add(1);

                    // panel_id: must exist and be a conversation history panel
                    let Some(id) = panel_obj.get("panel_id").and_then(|v| v.as_str()) else {
                        pf.errors.push(format!("Panel #{idx}: missing 'panel_id'"));
                        continue;
                    };

                    match state.context.iter().find(|c| c.id == id) {
                        None => pf.errors.push(format!("Panel #{idx}: '{id}' not found")),
                        Some(ctx) if ctx.context_type.as_str() != Kind::CONVERSATION_HISTORY => {
                            pf.errors.push(format!(
                                "Panel #{idx}: '{id}' is not a conversation history panel — use Close_panel instead"
                            ));
                        }
                        _ => {}
                    }

                    // Logs: require at least one non-empty string per panel
                    let has_logs = panel_obj
                        .get("logs")
                        .and_then(|v| v.as_array())
                        .is_some_and(|arr| arr.iter().any(|e| e.as_str().is_some_and(|s| !s.is_empty())));
                    if !has_logs {
                        pf.errors.push(format!(
                            "Panel #{idx} ('{id}'): provide at least one log entry to preserve context."
                        ));
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
            "Close_conversation_history" => Some(tools::execute_close_conversation_history(tool, state)),
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![("log_create", visualize_logs_output), ("Close_conversation_history", visualize_logs_output)]
    }

    fn create_panel(&self, _context_type: &Kind) -> Option<Box<dyn Panel>> {
        // No fixed panel — logs are searched via the search module.
        None
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![]
    }

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![]
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

    fn on_stream_chunk(&self, _text: &str, _state: &mut State) {}

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
            } else if line.starts_with('L') && line.chars().nth(1).is_some_and(|c| c.is_ascii_digit()) {
                Semantic::Info
            } else {
                Semantic::Default
            };
            let display = if line.len() > width {
                format!("{}...", line.get(..line.floor_char_boundary(width.saturating_sub(3))).unwrap_or(""))
            } else {
                line.to_owned()
            };
            Block::Line(vec![Span::styled(display, semantic)])
        })
        .collect()
}
