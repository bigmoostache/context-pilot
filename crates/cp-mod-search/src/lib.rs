//! Meilisearch-powered search module for Context Pilot.
//!
//! Provides full-text search across project files and logs via an embedded
//! Meilisearch server. Files are chunked using tree-sitter AST parsing
//! (with a fixed-size fallback) and indexed in the background.
//!
//! One tool: `search` — queries both file and log indexes.
//! Results appear as dynamic search result panels.

/// File-indexing pipeline: filters, background indexer, reconciliation.
pub mod index;
/// Meilisearch HTTP client, server lifecycle, and binary download.
pub mod meili;
/// Dynamic search result panel rendering and creation.
pub mod panel;
/// Context Radar — automatic log recall from Think task signals.
pub mod radar;
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

/// Pre-start the global Meilisearch server for parallel boot.
///
/// Spawns or reconnects to the Meilisearch daemon, waiting for it to
/// become healthy.  Called from background threads during boot to
/// overlap daemon startup with other module initialization.  When
/// `load_module_data()` later calls `ensure_server_running()`, it
/// finds the daemon already alive and reconnects instantly.
///
/// # Errors
///
/// Returns an error if the server cannot be started (download failure,
/// spawn error, health timeout).
pub fn pre_start_daemon() -> Result<(), String> {
    meili::server::ensure_server_running().map(|_info| ())
}

/// Read overlay information for the Ctrl+I overlay.
///
/// Delegates to [`meili::overlay::overlay_info`]. Returns `None` if the
/// search module hasn't been initialized.
#[must_use]
pub fn overlay_info(state: &State) -> Option<types::SearchOverlayInfo> {
    meili::overlay::overlay_info(state)
}

/// Get the Meilisearch server credentials (port, master key).
///
/// Returns `None` if the search module isn't initialized or the server
/// isn't running (port == 0). Used by other modules (e.g. entities) to
/// connect to the shared Meilisearch instance.
#[must_use]
pub fn meili_credentials(state: &State) -> Option<(u16, String)> {
    let ss = state.get_ext::<SearchState>()?;
    if ss.persist.port == 0 {
        return None;
    }
    Some((ss.persist.port, ss.persist.master_key.clone()))
}

/// Get the project hash used for per-project Meilisearch index naming.
///
/// Returns `None` if the search module isn't initialized. The hash is
/// an 8-character hex string derived from the project root path.
#[must_use]
pub fn project_hash(state: &State) -> Option<String> {
    let ss = state.get_ext::<SearchState>()?;
    if ss.persist.project_hash.is_empty() {
        return None;
    }
    Some(ss.persist.project_hash.clone())
}

/// Connect to (or start) the global Meilisearch server and stamp the resolved
/// port/master-key onto `persist`. Registers the project for orphan cleanup and
/// prunes stale indexes. On failure, zeroes the port (keyword-less fallback).
use index::boot::{bootstrap_server, compute_boot_plan, spawn_indexer_pipeline};

/// Lazily-loaded tool description texts parsed from the YAML definition file.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/search.yaml")));

/// Meilisearch-powered search module.
///
/// Manages an embedded Meilisearch server, background file indexer,
/// and a unified `search` tool for querying project files and logs.
#[derive(Debug, Clone, Copy)]
pub struct SearchModule;

impl Default for SearchModule {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchModule {
    /// Construct the module marker (funnels cross-crate construction of this
    /// `non_exhaustive` unit struct through an associated fn).
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

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
        vec![
            TypeMeta {
                context_type: "search_result",
                icon_id: "search",
                is_fixed: false,
                needs_cache: false,
                fixed_order: None,
                display_name: "search",
                short_name: "search",
                needs_async_wait: false,
            },
            TypeMeta {
                context_type: radar::RADAR_PANEL_TYPE,
                icon_id: "radar",
                is_fixed: true,
                needs_cache: false,
                fixed_order: Some(6),
                display_name: "radar",
                short_name: "radar",
                needs_async_wait: false,
            },
        ]
    }

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new("search_result")]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            panel::SEARCH_PANEL_TYPE => Some(Box::new(panel::SearchResultPanel)),
            radar::RADAR_PANEL_TYPE => Some(Box::new(radar::ContextRadarPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("search", t)
                .short_desc("Search across project files and logs using full-text search")
                .category("Search")
                .param("query", ParamType::String, true)
                .param("semantic_query", ParamType::String, true)
                .param_enum("scope", &["all", "project", "logs", "entities"], false)
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

    fn init_state(&self, state: &mut State) {
        // Lightweight defaults only — heavy server startup is deferred to
        // load_module_data() which runs for both fresh-start and reload paths.
        // This avoids the double-init problem where init_state() did expensive
        // work that load_module_data() immediately discarded and redid.
        state.set_ext(SearchState {
            persist: SearchPersistData::default(),
            indexer_tx: None,
            watcher: None,
            metrics: std::sync::Arc::new(std::sync::Mutex::new(types::SearchMetrics::default())),
            radar_cache: std::sync::Arc::new(std::sync::Mutex::new(types::RadarCache::default())),
            watchdog: None,
            backup_tick: None,
        });
    }

    fn reset_state(&self, state: &mut State) {
        // Lightweight reset — same as init_state
        self.init_state(state);
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let Some(ss) = state.get_ext::<SearchState>() else {
            return serde_json::Value::Null;
        };
        // Snapshot metrics into persist so they survive TUI reload
        let mut persist = ss.persist.clone();
        if let Ok(m) = ss.metrics.lock() {
            persist.recompute_counts.clone_from(&m.recompute_counts);
            persist.last_sent_ms.clone_from(&m.last_sent_ms);
        }
        serde_json::to_value(&persist).unwrap_or(serde_json::Value::Null)
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        // Unified bootstrap: handles both fresh-start (Null data) and reload
        // (persisted data) paths. This is the ONLY place heavy I/O happens.
        let mut persist = serde_json::from_value::<SearchPersistData>(data.clone()).unwrap_or_default();

        // Sanitize persisted signals — earlier versions could store leaked
        // thought_body content.  Truncate + strip XML artifacts.
        for sig in &mut persist.task_signals {
            sig.content = radar::sanitize_signal(&sig.content);
        }

        let project_path = std::env::current_dir().unwrap_or_default().to_string_lossy().to_string();
        let project_hash = meili::bootstrap::hash_project_path(&project_path);
        persist.project_hash.clone_from(&project_hash);

        // Start/reconnect the server, ensure indexes exist (stamps port + key).
        bootstrap_server(&mut persist, &project_path);

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

        // Restore recompute tracking from persisted data
        if let Ok(mut m) = metrics.lock() {
            m.recompute_counts.clone_from(&persist.recompute_counts);
            m.last_sent_ms.clone_from(&persist.last_sent_ms);
        }

        // Boot order (load-bearing): ensure_indexes → reimport-on-empty →
        // reconcile → watcher. The plan is computed against a quiesced index
        // BEFORE the live watcher goes up (boot disk is stable).
        let reconcile_plan = compute_boot_plan(&persist, &project_path);

        // Start indexer + watcher, inject the offline delta, mark scan complete.
        let (indexer_tx, watcher) = spawn_indexer_pipeline(&persist, &project_path, &metrics, reconcile_plan.as_ref());

        // Spawn the per-agent Meilisearch watchdog (only if the server is up).
        // It health-checks every few seconds and respawns the global server on
        // the SAME port if it dies mid-session, so a deployment self-heals with
        // no manual restart. Stored in SearchState so a reload drops+replaces it
        // (its Drop stops the old thread — never stacks two watchdogs).
        let watchdog = (persist.port > 0)
            .then(|| meili::watchdog::WatchdogHandle::spawn(persist.port, persist.master_key.clone()));

        // Hourly reconcile + embedding-backup tick — only when the server is up
        // AND the indexer channel exists (the tick queues its reconcile delta
        // through it). Dropped on reload so a reload never stacks two tickers.
        let backup_tick = match (persist.port > 0).then_some(()).and_then(|()| indexer_tx.clone()) {
            Some(tx) => Some(index::tick::BackupTickHandle::spawn(index::tick::TickParams {
                port: persist.port,
                master_key: persist.master_key.clone(),
                project_hash: persist.project_hash.clone(),
                project_root: std::path::PathBuf::from(&project_path),
                indexer_tx: tx,
            })),
            None => None,
        };

        state.set_ext(SearchState {
            persist,
            indexer_tx,
            watcher,
            metrics,
            radar_cache: std::sync::Arc::new(std::sync::Mutex::new(types::RadarCache::default())),
            watchdog,
            backup_tick,
        });

        // Backfill: push any existing logs to Meilisearch (idempotent upsert)
        index::logsync::sync_logs_to_meilisearch(state);

        // Pre-populate Context Radar from persisted signals + logs
        radar::refresh(state);
    }

    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn pre_flight(&self, _tool: &ToolUse, _state: &State) -> Option<cp_base::tools::pre_flight::Verdict> {
        None
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(radar::RADAR_PANEL_TYPE)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![(Kind::new(radar::RADAR_PANEL_TYPE), "Context Radar", false)]
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, cp_base::modules::ToolVisualizer)> {
        vec![("search", panel::visualize_search_output)]
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
            return Some("Search: server not available\n".to_owned());
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

    fn on_stream_chunk(&self, _text: &str, _state: &mut State) {}

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

/// Recompute the Context Radar panel content.
///
/// Queries the Meilisearch logs index using task context signals from the
/// Think tool.  Called from the main binary's pipeline after:
/// - Think (with `task_context`)
/// - `log_create` / `Close_conversation_history`
/// - Boot pre-population (via `load_module_data`)
pub fn refresh_radar(state: &State) {
    radar::refresh(state);
}

/// Push a task context signal from the Think tool into the ring buffer.
///
/// Called from `pipeline.rs` after a Think tool executes with a
/// `task_context` parameter.  Caps the buffer at [`types::MAX_TASK_SIGNALS`].
pub fn push_task_signal(state: &mut State, content: &str) {
    radar::push_signal(state, content);
}
