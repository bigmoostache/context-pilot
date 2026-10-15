//! Logs module — timestamped entries and conversation history.
//!
//! Two tools: `log_create` (content + importance) and
//! `Close_conversation_history` (closes history panels, keeping their logs).
//! Logs are stored globally in chunked JSON files under
//! `.context-pilot/logs/` and indexed by the search module for full-text retrieval.

/// Change tracking for chunk files: only changed chunks are rewritten.
mod dirty;
/// Regression tests: importance parsing, migration, chunk loading.
#[cfg(test)]
mod tests;
/// Tool implementations: create, close conversation history.
mod tools;
use dirty::{chunk_fingerprint, clean_map, next_id_op};
/// Log state types: `LogEntry`, `LogsState`.
pub mod types;

use types::{Importance, LogEntry, LogsState};

use cp_base::cast::Safe as _;

/// Logs subdirectory (chunked JSON files, global across workers)
pub const LOGS_DIR: &str = "logs";

/// Number of log entries per chunk file
pub const LOGS_CHUNK_SIZE: usize = 1000;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

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
fn schema_marker_path(dir: &Path) -> PathBuf {
    dir.join(".schema_v2")
}

/// Stamp the logs directory with the v2 schema marker.
///
/// Never deletes anything: v2 entries are a superset-compatible reading of
/// older chunks (unknown fields are ignored, `importance` defaults to medium),
/// so existing history is kept as is.
fn migrate_if_needed(dir: &Path) {
    let marker = schema_marker_path(dir);
    if marker.exists() {
        return;
    }
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let _r = fs::write(&marker, "migrated");
    log::info!("Logs: stamped v2 schema marker (existing chunks kept)");
}

/// Build write operations for the log chunks changed since their last write
/// (CPU only — no I/O).
///
/// Logs are append-only, so a save normally re-serializes only the newest
/// chunk; `next_id.json` is emitted only when the counter moved. Marks the
/// returned files clean: callers must hand every op to a writer lane that
/// never drops it (the persistence writer's durable lane). A fresh process
/// starts with an empty memo, so its first save rewrites every chunk once.
#[must_use]
pub fn build_log_write_ops(logs: &[LogEntry], next_log_id: usize) -> Vec<(PathBuf, Vec<u8>)> {
    let dir = logs_dir();
    let mut ops = Vec::new();
    let Ok(mut clean) = clean_map().lock() else { return ops };
    let chunks = group_by_chunk(logs);

    // Build write op for each changed chunk (sorted by index for deterministic output)
    let mut sorted_chunk_keys: Vec<_> = chunks.keys().copied().collect();
    sorted_chunk_keys.sort_unstable();
    for idx in sorted_chunk_keys {
        let Some(chunk_logs) = chunks.get(&idx) else { continue };
        let fp = chunk_fingerprint(chunk_logs);
        if clean.get(&idx) == Some(&fp) {
            continue;
        }
        if let Ok(json) = serde_json::to_string_pretty(chunk_logs) {
            let _prev = clean.insert(idx, fp);
            ops.push((dir.join(format!("chunk_{idx}.json")), json.into_bytes()));
        }
    }

    ops.extend(next_id_op(&mut clean, &dir, next_log_id));
    ops
}

/// Group log entries by chunk index (entries with a malformed id are skipped).
fn group_by_chunk(logs: &[LogEntry]) -> HashMap<usize, Vec<&LogEntry>> {
    let mut chunks: HashMap<usize, Vec<&LogEntry>> = HashMap::new();
    for log in logs {
        if let Some(num) = log.id.strip_prefix('L').and_then(|n| n.parse::<usize>().ok()) {
            chunks.entry(chunk_index(num)).or_default().push(log);
        }
    }
    chunks
}

/// Load all logs from chunked JSON files in .context-pilot/logs/
/// Load all logs from the chunked JSON files in `dir`.
///
/// A chunk that cannot be read or parsed is renamed aside
/// (`chunk_N.json.corrupt-<ms>`) and reported, so the next save cannot
/// overwrite its bytes with a partial chunk and the history stays recoverable.
fn load_logs_chunked(dir: &Path) -> (Vec<LogEntry>, usize) {
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
    if let Ok(entries) = fs::read_dir(dir) {
        let mut chunk_files: Vec<(usize, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let path = e.path();
                let stem = path.file_stem()?.to_str()?;
                let idx = stem.strip_prefix("chunk_")?.parse::<usize>().ok()?;
                Some((idx, path))
            })
            .collect();
        chunk_files.sort_by_key(|entry| entry.0);

        for (_, path) in chunk_files {
            match fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|content| serde_json::from_str::<Vec<LogEntry>>(&content).map_err(|e| e.to_string()))
            {
                Ok(logs) => all_logs.extend(logs),
                Err(e) => quarantine_chunk(&path, &e),
            }
        }
    }

    // Sort by ID number for consistent ordering
    all_logs.sort_by_key(|l| l.id.strip_prefix('L').and_then(|n| n.parse::<usize>().ok()).unwrap_or(0));

    // Never hand out an id already on disk, even if next_id.json lags or is lost.
    let max_id = all_logs.last().and_then(|l| l.id.strip_prefix('L')?.parse::<usize>().ok()).unwrap_or(0);
    (all_logs, next_log_id.max(max_id.saturating_add(1)))
}

/// Move an unreadable chunk aside and report it (see [`load_logs_chunked`]).
fn quarantine_chunk(path: &Path, err: &str) {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let aside = path.with_extension(format!("json.corrupt-{stamp}"));
    match fs::rename(path, &aside) {
        Ok(()) => log::error!("Logs: unreadable chunk {} ({err}); moved to {}", path.display(), aside.display()),
        Err(e) => log::error!("Logs: unreadable chunk {} ({err}); could not move it aside: {e}", path.display()),
    }
}

/// Logs module: timestamped entries and conversation history management.
#[derive(Debug, Clone, Copy)]
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
        migrate_if_needed(&logs_dir());
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
        let dir = logs_dir();
        migrate_if_needed(&dir);

        // Load logs from chunked files on disk
        let (logs, next_log_id) = load_logs_chunked(&dir);
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
                            .enum_vals(&Importance::NAMES),
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
                        ToolParam::new(
                            "logs",
                            ParamType::Array(Box::new(ParamType::Object(vec![
                                ToolParam::new("content", ParamType::String).desc("Short, atomic log entry").required(),
                                ToolParam::new("importance", ParamType::String)
                                    .desc("Importance level (default medium)")
                                    .enum_vals(&Importance::NAMES),
                            ]))),
                        )
                        .desc("Log entries to create, one per fact"),
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
                    pf.errors.push("Empty 'panels' array \u{2014} provide at least one panel to close".to_owned());
                    return Some(pf);
                }

                for (i, panel_obj) in panels.iter().enumerate() {
                    let idx = i.saturating_add(1);

                    // panel_id: must exist and be a conversation history panel
                    let Some(id) = panel_obj.get("panel_id").and_then(|v| v.as_str()) else {
                        pf.errors.push(format!("Panel #{idx}: missing 'panel_id'"));
                        continue;
                    };

                    match state.agent().context.iter().find(|c| c.id == id) {
                        None => pf.errors.push(format!("Panel #{idx}: '{id}' not found")),
                        Some(ctx) if ctx.context_type.as_str() != Kind::CONVERSATION_HISTORY => {
                            pf.errors.push(format!(
                                "Panel #{idx}: '{id}' is not a conversation history panel — use Close_panel instead"
                            ));
                        }
                        _ => {}
                    }

                    // Logs: at least one non-blank, valid entry per panel
                    let items = panel_obj.get("logs").and_then(|v| v.as_array()).map_or(&[][..], Vec::as_slice);
                    match tools::parse_log_items(items) {
                        Ok(logs) if logs.is_empty() => pf.errors.push(format!(
                            "Panel #{idx} ('{id}'): provide at least one log entry to preserve context."
                        )),
                        Ok(_) => {}
                        Err(errors) => pf.errors.push(format!("Panel #{idx} ('{id}'): {}", errors.join("; "))),
                    }
                }

                Some(pf)
            }
            "log_create" => Some(tools::pre_flight_log_create(tool)),
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
