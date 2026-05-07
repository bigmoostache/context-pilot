//! Meilisearch-powered search module for Context Pilot.
//!
//! Provides full-text search across project files and logs via an embedded
//! Meilisearch server. Files are chunked using tree-sitter AST parsing
//! (with a fixed-size fallback) and indexed in the background.
//!
//! One tool: `search` — queries both file and log indexes.
//! Results appear as dynamic search result panels.

/// Meilisearch HTTP API client: index management, document CRUD, search.
pub mod client;
/// Background file indexer thread and file watcher.
pub mod indexer;
/// Dynamic search result panel rendering and creation.
pub mod panel;
/// Meilisearch server lifecycle: download, start, health check, reconnect.
pub mod server;
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

use types::{SearchOverlayInfo, SearchPersistData, SearchState};

/// Lazily-loaded tool description texts parsed from the YAML definition file.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/search.yaml")));

/// Compute an 8-character hex hash of a path for per-project index naming.
fn hash_project_path(path: &str) -> String {
    use sha2::Digest as _;
    let hash = sha2::Sha256::digest(path.as_bytes());
    // Take first 4 bytes → 8 hex chars
    hex_encode_4_bytes(hash.as_slice())
}

/// Encode the first 4 bytes of a slice as an 8-character lowercase hex string.
fn hex_encode_4_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(8);
    for &b in bytes.iter().take(4) {
        use std::fmt::Write as _;
        let _r = write!(out, "{b:02x}");
    }
    out
}

/// Create per-project Meilisearch indexes if they don't already exist.
///
/// Creates `cp_{hash}_files` and `cp_{hash}_logs` indexes with appropriate
/// settings (searchable, filterable, sortable attributes).
///
/// # Errors
///
/// Returns an error if any API call fails.
fn ensure_indexes(port: u16, master_key: &str, project_hash: &str) -> Result<(), String> {
    let meili = client::MeiliClient::new(port, master_key)?;

    let files_uid = format!("cp_{project_hash}_files");
    let logs_uid = format!("cp_{project_hash}_logs");

    // Files index
    if !meili.index_exists(&files_uid)? {
        let create_task = meili.create_index(&files_uid, "id")?;
        meili.wait_for_task(create_task)?;
        let settings_task = meili.update_settings(&files_uid, &types::files_index_settings())?;
        meili.wait_for_task(settings_task)?;
        log::info!("Created files index: {files_uid}");
    }

    // Logs index
    if !meili.index_exists(&logs_uid)? {
        let create_task = meili.create_index(&logs_uid, "id")?;
        meili.wait_for_task(create_task)?;
        let settings_task = meili.update_settings(&logs_uid, &types::logs_index_settings())?;
        meili.wait_for_task(settings_task)?;
        log::info!("Created logs index: {logs_uid}");
    }

    Ok(())
}

/// Read overlay information from the search module's state.
///
/// Returns `None` if the search module hasn't been initialized.
/// Used by the main binary's Ctrl+I overlay renderer.
#[must_use]
pub fn overlay_info(state: &State) -> Option<SearchOverlayInfo> {
    let ss = state.get_ext::<SearchState>()?;
    let (chunks, files, queue_depth, error_count, last_activity_ms) = {
        let metrics = ss.metrics.lock().ok()?;
        (
            metrics.chunks_indexed,
            metrics.files_indexed,
            metrics.queue_depth,
            metrics.error_count,
            metrics.last_activity_ms,
        )
    };
    Some(SearchOverlayInfo {
        port: ss.persist.port,
        chunks_indexed: chunks,
        files_indexed: files,
        queue_depth,
        error_count,
        last_activity_ms,
        index_ready: ss.persist.index_ready,
    })
}

/// Query Meilisearch for initial index statistics and populate metrics.
///
/// Called once during `init_state` / `load_module_data` so the Ctrl+I overlay
/// shows correct counts immediately (before the indexer has done any work).
fn populate_initial_metrics(
    port: u16,
    master_key: &str,
    project_hash: &str,
    metrics: &std::sync::Arc<std::sync::Mutex<types::SearchMetrics>>,
) {
    let Ok(meili) = client::MeiliClient::new(port, master_key) else {
        return;
    };

    let files_uid = format!("cp_{project_hash}_files");
    let logs_uid = format!("cp_{project_hash}_logs");

    let (mut chunks, files) = if let Ok((count, _indexing)) = meili.index_stats(&files_uid) {
        let f = count.checked_div(3).unwrap_or(0).max(u64::from(count > 0));
        (count, f)
    } else {
        (0, 0)
    };

    // Also count logs (optional — just for awareness)
    if let Ok((log_count, _)) = meili.index_stats(&logs_uid) {
        chunks = chunks.saturating_add(log_count);
    }

    if let Ok(mut m) = metrics.lock() {
        m.chunks_indexed = chunks;
        m.files_indexed = files;
    }
}

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
                .param("include_context", ParamType::Boolean, false)
                .param("limit", ParamType::Integer, false)
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
        let project_hash = hash_project_path(&project_path);

        // Try to start/reconnect to the global Meilisearch server
        let (port, master_key) = match server::ensure_server_running() {
            Ok(info) => {
                // Register this project for orphan cleanup
                let _r = server::register_project(&project_path, &project_hash);
                // Clean up stale indexes from deleted projects
                server::cleanup_orphan_indexes(info.port, &info.master_key);
                (info.port, info.master_key)
            }
            Err(e) => {
                log::warn!("Meilisearch server not available: {e}");
                (0, String::new())
            }
        };

        // Create per-project indexes if the server is available
        if port > 0
            && let Err(e) = ensure_indexes(port, &master_key, &project_hash)
        {
            log::warn!("Failed to create Meilisearch indexes: {e}");
        }

        // Start background indexer + file watcher
        let metrics = std::sync::Arc::new(std::sync::Mutex::new(types::SearchMetrics::default()));

        // Populate initial metrics from existing Meilisearch indexes
        if port > 0 {
            populate_initial_metrics(port, &master_key, &project_hash, &metrics);
        }

        let (indexer_tx, watcher) = if port > 0 {
            match indexer::start(indexer::IndexerParams {
                port,
                master_key: master_key.clone(),
                project_hash: project_hash.clone(),
                project_root: std::path::PathBuf::from(&project_path),
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

        let persist = SearchPersistData { port, master_key, project_hash, index_ready: false };

        state.set_ext(SearchState { persist, indexer_tx, watcher, metrics });
    }

    fn reset_state(&self, state: &mut State) {
        self.init_state(state);
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        state
            .get_ext::<SearchState>()
            .and_then(|s| serde_json::to_value(&s.persist).ok())
            .unwrap_or(serde_json::Value::Null)
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Ok(persist) = serde_json::from_value::<SearchPersistData>(data.clone()) {
            let metrics = std::sync::Arc::new(std::sync::Mutex::new(types::SearchMetrics::default()));

            // Populate initial metrics from existing Meilisearch indexes
            if persist.port > 0 {
                populate_initial_metrics(persist.port, &persist.master_key, &persist.project_hash, &metrics);
            }

            // Restart indexer + watcher if the server was available
            let (indexer_tx, watcher) = if persist.port > 0 {
                let project_path = std::env::current_dir().unwrap_or_default().to_string_lossy().to_string();
                match indexer::start(indexer::IndexerParams {
                    port: persist.port,
                    master_key: persist.master_key.clone(),
                    project_hash: persist.project_hash.clone(),
                    project_root: std::path::PathBuf::from(&project_path),
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

        let (chunks, files) = {
            let metrics = ss.metrics.lock().ok()?;
            (metrics.chunks_indexed, metrics.files_indexed)
        };
        let status = if ss.persist.index_ready { "ready" } else { "indexing" };

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
