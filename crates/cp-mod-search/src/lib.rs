//! Meilisearch-powered search module for Context Pilot.
//!
//! Provides full-text search across project files and logs via an embedded
//! Meilisearch server. Files are chunked using tree-sitter AST parsing
//! (with a fixed-size fallback) and indexed in the background.
//!
//! One tool: `search` — queries both file and log indexes.
//! Results appear as dynamic search result panels.

/// Background file indexer thread and file watcher.
pub mod indexer;
/// Meilisearch HTTP client, server lifecycle, and binary download.
pub mod meili;
/// Datalab OCR API client for converting PDFs/images to text.
pub mod ocr;
/// Dynamic search result panel rendering and creation.
pub mod panel;
/// File content splitter chain (fixed-size fallback, future tree-sitter).
pub mod splitter;
/// Search tool dispatch and execution.
pub mod tools;
/// Core data types: `SearchState`, `SearchPersistData`, etc.
pub mod types;

use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::context::{Kind, TypeMeta};
use cp_base::state::runtime::State;
use cp_base::tools::{ParamType, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use types::{SearchPersistData, SearchState};

/// Read overlay information for the Ctrl+I overlay.
///
/// Delegates to [`meili::overlay::overlay_info`]. Returns `None` if the
/// search module hasn't been initialized.
#[must_use]
pub fn overlay_info(state: &State) -> Option<types::SearchOverlayInfo> {
    meili::overlay::overlay_info(state)
}

/// Lazily-loaded tool description texts parsed from the YAML definition file.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/search.yaml")));

/// Meilisearch-powered search module.
///
/// Manages an embedded Meilisearch server, background file indexer,
/// and a unified `search` tool for querying project files and logs.
#[derive(Debug, Clone, Copy)]
pub struct SearchModule;

impl Module for SearchModule {
    fn id(&self) -> &'static str {
        "search"
    }

    fn name(&self) -> &'static str {
        "Search"
    }

    fn description(&self) -> &'static str {
        "Full-text search across project files and logs via Meilisearch"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["core"]
    }

    fn is_global(&self) -> bool {
        false
    }

    fn is_core(&self) -> bool {
        false
    }

    fn context_type_metadata(&self) -> Vec<TypeMeta> {
        vec![TypeMeta {
            context_type: "search_result",
            icon_id: "search",
            is_fixed: false,
            needs_cache: false,
            fixed_order: None,
            display_name: "search",
            short_name: "search",
            needs_async_wait: false,
        }]
    }

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new("search_result")]
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("search", t)
                .short_desc("Search across project files and logs using full-text search")
                .category("Search")
                .param("query", ParamType::String, true)
                .param_enum("scope", &["all", "project", "logs"], false)
                .param("path_prefix", ParamType::String, false)
                .param("extension", ParamType::String, false)
                .param_enum("sort", &["relevance", "date_asc", "date_desc"], false)
                .param("from_date", ParamType::String, false)
                .param("to_date", ParamType::String, false)
                .param("limit", ParamType::Integer, false)
                .param("semantic_ratio", ParamType::Number, false)
                .param("hide_contents", ParamType::Boolean, false)
                .build(),
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        tools::dispatch(tool, state)
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        (context_type.as_str() == panel::SEARCH_PANEL_TYPE).then(|| {
            let p: Box<dyn Panel> = Box::new(panel::SearchResultPanel);
            p
        })
    }

    fn init_state(&self, state: &mut State) {
        let project_path = std::env::current_dir().unwrap_or_default().to_string_lossy().to_string();
        let project_hash = meili::bootstrap::hash_project_path(&project_path);

        // Try to start/reconnect to the global Meilisearch server
        let (port, master_key) = match meili::server::ensure_server_running() {
            Ok(info) => {
                // Register this project for orphan cleanup
                let _r = meili::server::register_project(&project_path, &project_hash);
                // Clean up stale indexes from deleted projects
                meili::server::cleanup_orphan_indexes(info.port, &info.master_key);
                (info.port, info.master_key)
            }
            Err(e) => {
                log::warn!("Meilisearch server not available: {e}");
                (0, String::new())
            }
        };

        // Create per-project indexes if the server is available
        if port > 0
            && let Err(e) = meili::bootstrap::ensure_indexes(port, &master_key, &project_hash)
        {
            log::warn!("Failed to create Meilisearch indexes: {e}");
        }

        // Start background indexer + file watcher
        let metrics = std::sync::Arc::new(std::sync::Mutex::new(types::SearchMetrics::default()));

        // Populate initial metrics from existing Meilisearch indexes
        if port > 0 {
            meili::bootstrap::populate_initial_metrics(port, &master_key, &project_hash, &metrics);
        }

        let (indexer_tx, watcher) = if port > 0 {
            match indexer::start(indexer::IndexerParams {
                port,
                master_key: master_key.clone(),
                project_hash: project_hash.clone(),
                project_root: std::path::PathBuf::from(&project_path),
                metrics: std::sync::Arc::clone(&metrics),
                skip_initial_scan: false,
            }) {
                Ok((tx, w)) => (Some(tx), Some(types::WatcherHandle::new(w))),
                Err(e) => {
                    log::warn!("Failed to start search indexer: {e}");
                    (None, None)
                }
            }
        } else {
            (None, None)
        };

        let persist =
            SearchPersistData { port, master_key, project_hash, index_ready: false, ..SearchPersistData::default() };

        state.set_ext(SearchState { persist, indexer_tx, watcher, metrics });
    }

    fn reset_state(&self, state: &mut State) {
        self.init_state(state);
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let Some(ss) = state.get_ext::<SearchState>() else {
            return serde_json::Value::Null;
        };
        // Snapshot OCR metrics into persist so they survive TUI reload
        let mut persist = ss.persist.clone();
        if let Ok(m) = ss.metrics.lock() {
            persist.ocr_attempted = m.ocr_attempted;
            persist.ocr_succeeded = m.ocr_succeeded;
            persist.ocr_failed = m.ocr_failed;
            persist.ocr_cached = m.ocr_cached;
            persist.recompute_counts.clone_from(&m.recompute_counts);
            persist.last_sent_ms.clone_from(&m.last_sent_ms);
        }
        serde_json::to_value(&persist).unwrap_or(serde_json::Value::Null)
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Ok(mut persist) = serde_json::from_value::<SearchPersistData>(data.clone()) {
            // Re-validate server connection — the port may have changed if the
            // Meilisearch process was killed and restarted between saves.
            match meili::server::ensure_server_running() {
                Ok(info) => {
                    persist.port = info.port;
                    persist.master_key = info.master_key;
                }
                Err(e) => {
                    log::warn!("Meilisearch server not available on reload: {e}");
                    persist.port = 0;
                    persist.master_key = String::new();
                }
            }

            // Ensure indexes + embedders exist (idempotent — skips if already configured).
            // Needed because embedder settings may have been removed or the server
            // was wiped between saves.
            if persist.port > 0
                && let Err(e) =
                    meili::bootstrap::ensure_indexes(persist.port, &persist.master_key, &persist.project_hash)
            {
                log::warn!("Failed to ensure Meilisearch indexes on reload: {e}");
            }

            let metrics = std::sync::Arc::new(std::sync::Mutex::new(types::SearchMetrics::default()));

            // Populate initial metrics from existing Meilisearch indexes
            if persist.port > 0 {
                meili::bootstrap::populate_initial_metrics(
                    persist.port,
                    &persist.master_key,
                    &persist.project_hash,
                    &metrics,
                );
            }

            // Restore OCR metrics from persisted data (not stored in Meilisearch).
            // Only overwrite if persist has real values — otherwise keep the
            // inferred values from populate_initial_metrics (which checks the
            // disk cache and Meilisearch facets as a fallback).
            if let Ok(mut m) = metrics.lock()
                && persist.ocr_attempted > 0
            {
                m.ocr_attempted = persist.ocr_attempted;
                m.ocr_succeeded = persist.ocr_succeeded;
                m.ocr_failed = persist.ocr_failed;
                m.ocr_cached = persist.ocr_cached;
                m.ocr_enabled = true;
            }

            // Restore recompute tracking from persisted data
            if let Ok(mut m) = metrics.lock() {
                m.recompute_counts.clone_from(&persist.recompute_counts);
                m.last_sent_ms.clone_from(&persist.last_sent_ms);
            }

            // Restart indexer + watcher if the server was available
            let (indexer_tx, watcher) = if persist.port > 0 {
                let project_path = std::env::current_dir().unwrap_or_default().to_string_lossy().to_string();
                match indexer::start(indexer::IndexerParams {
                    port: persist.port,
                    master_key: persist.master_key.clone(),
                    project_hash: persist.project_hash.clone(),
                    project_root: std::path::PathBuf::from(&project_path),
                    metrics: std::sync::Arc::clone(&metrics),
                    skip_initial_scan: true,
                }) {
                    Ok((tx, w)) => (Some(tx), Some(types::WatcherHandle::new(w))),
                    Err(e) => {
                        log::warn!("Failed to restart search indexer: {e}");
                        (None, None)
                    }
                }
            } else {
                (None, None)
            };

            state.set_ext(SearchState { persist, indexer_tx, watcher, metrics });

            // Backfill: push any existing logs to Meilisearch (idempotent upsert)
            sync_logs_to_meilisearch(state);
        }
    }

    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn pre_flight(&self, _tool: &ToolUse, _state: &State) -> Option<cp_base::tools::pre_flight::Verdict> {
        None
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![]
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, cp_base::modules::ToolVisualizer)> {
        vec![("search", visualize_search_output)]
    }

    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
        None
    }

    fn context_detail(&self, _ctx: &cp_base::state::context::Entry) -> Option<String> {
        None
    }

    fn overview_context_section(&self, state: &State) -> Option<String> {
        let ss = state.get_ext::<SearchState>()?;
        let port = ss.persist.port;

        if port == 0 {
            return Some("Search: server not available\n".to_string());
        }

        let (chunks, files, scan_complete) = {
            let metrics = ss.metrics.lock().ok()?;
            (metrics.chunks_indexed, metrics.files_indexed, metrics.scan_complete)
        };
        let status = if scan_complete { "ready" } else { "indexing" };

        Some(format!("Search: {chunks} chunks indexed across ~{files} files (port {port}, {status})\n"))
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

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Search", "Full-text search via Meilisearch")]
    }

    fn on_user_message(&self, _state: &mut State) {}

    fn on_stream_stop(&self, _state: &mut State) {}

    fn on_tool_progress(&self, _tool_name: &str, _input_so_far: &str, _state: &mut State) {}

    fn on_tool_complete(&self, _tool_name: &str, _state: &mut State) {}

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
}

/// Push all log entries from the logs module into the Meilisearch logs index.
///
/// Uses upsert semantics — existing documents with the same ID are updated,
/// new ones are inserted.  Cheap for the typical log volume (~hundreds).
///
/// Called from:
/// - `load_module_data()` to backfill existing logs on boot/reload.
/// - `handle_tool_execution()` in the main binary after `log_create` /
///   `Close_conversation_history` finish executing (the `on_tool_complete`
///   hook fires too early — during streaming, before execution).
pub fn sync_logs_to_meilisearch(state: &State) {
    let Some(ss) = state.get_ext::<SearchState>() else { return };
    if ss.persist.port == 0 {
        return;
    }
    let port = ss.persist.port;
    let master_key = ss.persist.master_key.clone();
    let logs_uid = format!("cp_{}_logs", ss.persist.project_hash);

    let ls = cp_mod_logs::types::LogsState::get(state);
    if ls.logs.is_empty() {
        return;
    }

    let docs: Vec<serde_json::Value> = ls
        .logs
        .iter()
        .map(|l| {
            serde_json::json!({
                "id": l.id,
                "content": l.content,
                "importance": l.importance,
                "tags": l.tags,
                "timestamp_ms": l.timestamp_ms,
                "datetime": l.datetime,
            })
        })
        .collect();

    let Ok(client) = meili::client::MeiliClient::new(port, &master_key) else { return };
    if let Ok(task) = client.add_documents(&logs_uid, &serde_json::Value::Array(docs)) {
        let _r = client.wait_for_task(task);
    }
}

/// Visualizer for search tool results.
///
/// Highlights file paths, section headers, importance levels, and tags
/// in the conversation view.
fn visualize_search_output(content: &str, width: usize) -> Vec<cp_render::Block> {
    use cp_render::{Block, Semantic, Span};

    content
        .lines()
        .map(|line| {
            if line.is_empty() {
                return Block::empty();
            }

            // Truncate long lines
            let display = if line.len() > width {
                format!("{}...", line.get(..line.floor_char_boundary(width.saturating_sub(3))).unwrap_or(""))
            } else {
                line.to_string()
            };

            let semantic = if line.starts_with("Results for") || line.starts_with("No results") {
                Semantic::Info
            } else if line.starts_with("---") && line.ends_with("---") {
                Semantic::Header
            } else if line.starts_with("Error") || line.contains("[critical]") {
                Semantic::Error
            } else if line.contains("[high]") {
                Semantic::Warning
            } else if line.contains("[low]") {
                Semantic::Muted
            } else if line.starts_with(|c: char| c.is_ascii_digit()) && line.contains(":[") {
                // File result line like "1. src/main.rs:15-42 [function: run]"
                Semantic::Success
            } else {
                Semantic::Default
            };

            Block::Line(vec![Span::styled(display, semantic)])
        })
        .collect()
}
