//! Core types for the search module.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};

use serde::{Deserialize, Serialize};

/// A task context signal from the Think tool, used as a Context Radar query.
///
/// Stored in a ring buffer (cap 20) in [`SearchPersistData`].
/// Each signal describes *what* the AI is working on — used as a semantic
/// search query against the logs index for automatic recall.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TaskSignal {
    /// Unix timestamp (ms) when this signal was created.
    pub timestamp_ms: u64,
    /// Total log count at the time this signal was pushed.
    /// Used for log-count-based decay (distance = `current_count` - this).
    #[serde(default)]
    pub log_count: u64,
    /// Short description of the current task (1–2 sentences).
    pub content: String,
}

/// Maximum number of task signals to keep in the ring buffer.
pub(crate) const MAX_TASK_SIGNALS: usize = 20;

/// Cached Context Radar panel content.
///
/// Updated by [`crate::radar::refresh`] on a background thread after
/// Think (with `task_context`) or log creation.  Read by
/// [`crate::radar::ContextRadarPanel::context_content`].
///
/// Wrapped in `Arc<Mutex<>>` so a background refresh thread can write
/// results without blocking the main event loop.
#[derive(Debug, Clone, Default)]
pub(crate) struct RadarCache {
    /// Pre-computed YAML content for the panel.
    pub yaml: String,
    /// Unix timestamp (ms) of last refresh.
    pub last_refresh_ms: u64,
}

/// Thread-safe handle to the radar cache, shared between the main thread
/// and background refresh jobs.
pub(crate) type SharedRadarCache = Arc<Mutex<RadarCache>>;

/// Persisted search state — survives TUI reloads.
///
/// Serialized via `save_module_data` / `load_module_data`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SearchPersistData {
    /// TCP port the Meilisearch server is listening on.
    pub port: u16,
    /// API master key for authenticating with Meilisearch.
    pub master_key: String,
    /// 8-character hash of the project root path (for per-project index naming).
    pub project_hash: String,
    /// Whether the initial full-project indexing has completed.
    pub index_ready: bool,
    /// Per-file recompute counter: how many times each file has been re-indexed.
    /// Persisted across TUI reloads to accumulate counts over time.
    #[serde(default)]
    pub recompute_counts: HashMap<String, u64>,
    /// Per-file last-sent timestamp (ms since epoch).
    /// Persisted so "recently recomputed" survives reloads.
    #[serde(default)]
    pub last_sent_ms: HashMap<String, u64>,
    /// Task context signals from the Think tool — ring buffer, cap [`MAX_TASK_SIGNALS`].
    /// Used by Context Radar to query the logs index for automatic recall.
    #[serde(default)]
    pub task_signals: Vec<TaskSignal>,
}

/// Full runtime search state stored in the `State` `TypeMap`.
///
/// Contains the persisted data plus runtime-only handles for the
/// background indexer thread and file watcher.
pub(crate) struct SearchState {
    /// Persisted fields that survive TUI reloads.
    pub persist: SearchPersistData,
    /// Channel to send commands to the background indexer thread.
    /// `None` if the indexer hasn't been started (e.g., server unavailable).
    pub indexer_tx: Option<mpsc::Sender<IndexerCmd>>,
    /// File system watcher handle.  Must stay alive — drop stops watching.
    /// Wrapped in `Option` so it can be taken/replaced on reload.
    pub watcher: Option<WatcherHandle>,
    /// Indexer metrics, shared with the indexer thread via `Arc`.
    /// Read by the Ctrl+I overlay and overview panel.
    pub metrics: Arc<Mutex<SearchMetrics>>,
    /// Cached Context Radar panel content.  Shared with background refresh
    /// threads via `Arc<Mutex<>>`.  Updated by [`crate::radar::refresh`].
    pub radar_cache: SharedRadarCache,
    /// Per-agent Meilisearch supervision thread. `None` when the server never
    /// came up (port 0). Dropping it (on reload) stops the old watchdog so a
    /// reload never stacks a second supervisor on the same global server.
    pub watchdog: Option<super::meili::watchdog::WatchdogHandle>,
    /// Hourly reconcile + embedding-backup tick thread. `None` when the server
    /// never came up. Dropped on reload so a reload never stacks two tickers.
    pub backup_tick: Option<super::index::tick::BackupTickHandle>,
}

impl std::fmt::Debug for SearchState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchState")
            .field("persist", &self.persist)
            .field("indexer_tx", &self.indexer_tx.is_some())
            .field("watcher", &self.watcher.is_some())
            .field("metrics", &self.metrics)
            .field("radar_cache", &self.radar_cache)
            .field("watchdog", &self.watchdog)
            .field("backup_tick", &self.backup_tick)
            .finish()
    }
}

/// Newtype wrapper around [`notify::RecommendedWatcher`].
///
/// `RecommendedWatcher` maps to the platform-native backend (`FSEvents` on
/// macOS, inotify on Linux) — event-driven, negligible CPU, typically a
/// single kernel FD.
///
/// `RecommendedWatcher` does not implement `Debug`, so we wrap it
/// to satisfy the `Debug` requirement for types stored in the `TypeMap`.
/// Wrapped in a `Mutex` to satisfy `Sync` without `unsafe`.
pub(crate) struct WatcherHandle {
    /// The inner watcher, wrapped in a `Mutex` for `Sync`.
    _inner: Mutex<notify::RecommendedWatcher>,
}

impl WatcherHandle {
    /// Wrap a watcher for storage in the `TypeMap`. The handle
    /// must stay alive — dropping it stops file watching.
    pub(crate) const fn new(w: notify::RecommendedWatcher) -> Self {
        Self { _inner: Mutex::new(w) }
    }
}

impl std::fmt::Debug for WatcherHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WatcherHandle(..)")
    }
}

/// A chunk of file content produced by the splitter.
///
/// Represents a semantic unit (function, struct, class) or a fixed-size
/// raw block when AST parsing is unavailable.
#[derive(Debug, Clone)]
pub(crate) struct Chunk {
    /// The text content of this chunk.
    pub content: String,
    /// Semantic type: `"function"`, `"struct"`, `"class"`, `"raw"`, etc.
    pub kind: String,
    /// Name of the semantic unit, or empty for raw chunks.
    pub name: String,
    /// 1-based start line in the original file.
    pub line_start: u32,
    /// 1-based end line in the original file.
    pub line_end: u32,
    /// Character offset (0-based) of the chunk start.
    pub char_start: u32,
    /// Character offset (0-based) of the chunk end.
    pub char_end: u32,
}

/// A recent Meilisearch task (for the overlay "Recent Tasks" section).
#[derive(Debug, Clone)]
pub(crate) struct MeiliTask {
    /// Task UID.
    pub uid: u64,
    /// Task type (e.g. "documentAdditionOrUpdate", "settingsUpdate").
    pub task_type: String,
    /// Task status ("succeeded", "processing", "enqueued", "failed").
    pub status: String,
    /// ISO 8601 duration (e.g. "PT0.254092S") or empty if still running.
    pub duration: String,
}

/// Live statistics fetched from the Meilisearch `/stats` endpoint.
///
/// Cached in `SearchMetrics` and refreshed at most every 2 seconds
/// to avoid hammering the server from the Ctrl+I overlay render loop.
#[derive(Debug, Clone, Default)]
pub(crate) struct MeiliLiveStats {
    /// Total database size on disk (bytes).
    pub database_size_bytes: u64,
    /// Used portion of the database (bytes).
    pub used_database_size_bytes: u64,
    /// Number of embeddings in the files index.
    pub files_embedding_count: u64,
    /// Whether the files index is currently indexing/embedding.
    pub files_is_indexing: bool,
    /// Number of documents in the logs index.
    pub logs_doc_count: u64,
    /// Name of the configured embedding model (e.g. "BAAI/bge-base-en-v1.5").
    pub embedding_model: String,
    /// Unix timestamp (ms) when these stats were fetched.
    pub fetched_at_ms: u64,
    /// Meilisearch server version (e.g. "1.43.0").
    pub version: String,
    /// Average document size in bytes (files index).
    pub avg_document_size: u64,
    /// Raw document storage size in bytes (files index).
    pub raw_document_db_size: u64,
    /// Number of documents that have embeddings (files index).
    pub files_embedded_doc_count: u64,
    /// Total number of documents in the files index.
    pub files_total_doc_count: u64,
    /// ISO 8601 timestamp of last index update (from Meilisearch).
    pub last_update: String,
    /// Recent tasks (last 5, filtered to project indexes).
    pub recent_tasks: Vec<MeiliTask>,
    /// Meilisearch process CPU ticks (for delta computation across refreshes).
    pub meili_cpu_ticks: u64,
    /// Meilisearch process CPU usage percentage (computed from tick deltas).
    pub meili_cpu_pct: f32,
    /// Meilisearch process RSS in bytes.
    pub meili_memory_bytes: u64,
}

/// Runtime metrics for the background indexer.
///
/// Shared between the indexer thread and the module via `Arc<Mutex<…>>`.
/// Read by the Ctrl+I overlay and overview panel.
#[derive(Debug, Clone, Default)]
pub(crate) struct SearchMetrics {
    /// Number of file chunks currently in the Meilisearch index.
    pub chunks_indexed: u64,
    /// Number of files successfully indexed.
    pub files_indexed: u64,
    /// Number of indexing operations queued (pending debounce + processing).
    pub queue_depth: u64,
    /// Number of indexing errors since last successful connect.
    pub error_count: u64,
    /// Unix timestamp (ms) of the last indexing activity.
    pub last_activity_ms: u64,
    /// Per-extension file counts (e.g. "rs" → 142, "py" → 37).
    pub extension_counts: HashMap<String, u64>,
    /// Number of tree-sitter AST chunks produced.
    pub tree_sitter_chunks: u64,
    /// Number of fixed-size fallback chunks produced.
    pub fallback_chunks: u64,
    /// Whether the initial full-project scan has completed.
    pub scan_complete: bool,
    /// Cached live stats from the Meilisearch `/stats` endpoint.
    ///
    /// Refreshed at most every 2 seconds by `overlay_info()`.
    pub live_stats: Option<MeiliLiveStats>,
    /// Per-file recompute counter: how many times each relative path
    /// has been delete+re-indexed since project creation.
    pub recompute_counts: HashMap<String, u64>,
    /// Per-file last-sent timestamp (ms since epoch): when each file
    /// was most recently pushed to Meilisearch.
    pub last_sent_ms: HashMap<String, u64>,
}

/// Commands sent to the background indexer thread.
#[derive(Debug)]
pub(crate) enum IndexerCmd {
    /// Read, chunk, and index a file.
    IndexFile(PathBuf),
    /// Remove all indexed chunks for a file.
    DeleteFile(PathBuf),
    /// The initial full-project scan has finished.
    ScanComplete,
}

// -- Public overlay info -----------------------------------------------------

/// Information exposed to the main binary for the Ctrl+I overlay.
///
/// Constructed by [`crate::overlay_info`] from internal state.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SearchOverlayInfo {
    /// TCP port the Meilisearch server is listening on.
    pub port: u16,
    /// Number of file chunks currently indexed.
    pub chunks_indexed: u64,
    /// Number of files successfully indexed.
    pub files_indexed: u64,
    /// Number of indexing operations queued.
    pub queue_depth: u64,
    /// Number of indexing errors since last successful connect.
    pub error_count: u64,
    /// Unix timestamp (ms) of the last indexing activity.
    pub last_activity_ms: u64,
    /// Whether the initial full-project scan has completed.
    pub index_ready: bool,
    /// Top extensions by file count (sorted descending, max 8).
    pub top_extensions: Vec<(String, u64)>,
    /// Number of tree-sitter AST chunks produced.
    pub tree_sitter_chunks: u64,
    /// Number of fixed-size fallback chunks produced.
    pub fallback_chunks: u64,
    /// Total database size on disk (bytes). From live stats.
    pub database_size_bytes: u64,
    /// Used portion of the database (bytes). From live stats.
    pub used_database_size_bytes: u64,
    /// Number of embeddings in the files index. From live stats.
    pub files_embedding_count: u64,
    /// Whether the files index is currently indexing/embedding.
    pub files_is_indexing: bool,
    /// Number of documents in the logs index.
    pub logs_doc_count: u64,
    /// Name of the configured embedding model.
    pub embedding_model: String,
    /// Meilisearch server version (e.g. "1.43.0").
    pub meili_version: String,
    /// Average document size in bytes (files index).
    pub avg_document_size: u64,
    /// Raw document storage size in bytes (files index).
    pub raw_document_db_size: u64,
    /// Number of documents that have embeddings.
    pub files_embedded_doc_count: u64,
    /// Total number of documents in the files index.
    pub files_total_doc_count: u64,
    /// ISO 8601 timestamp of last index update.
    pub last_update: String,
    /// Recent Meilisearch tasks (last 5).
    pub recent_tasks: Vec<MeiliTaskInfo>,
    /// Top files by recompute count (sorted descending, max 8).
    pub top_recomputed: Vec<(String, u64)>,
    /// Most recently re-indexed files (sorted by timestamp descending, max 8).
    pub recently_sent: Vec<(String, u64)>,
    /// Meilisearch process CPU usage percentage.
    pub meili_cpu_pct: f32,
    /// Meilisearch process RSS in bytes.
    pub meili_memory_bytes: u64,
}

// -- Search results ----------------------------------------------------------

/// A recent Meilisearch task exposed to the overlay renderer.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MeiliTaskInfo {
    /// Task UID.
    pub uid: u64,
    /// Short task type label (e.g. "docAdd", "docDelete", "settings").
    pub task_type: String,
    /// Task status ("succeeded", "processing", "enqueued", "failed").
    pub status: String,
    /// Human-readable duration (e.g. "0.25s") or "—" if still running.
    pub duration: String,
}

/// A single search result, either a file chunk or a log entry.
///
/// Displayed in the dynamic search result panel.
#[derive(Debug, Clone)]
pub(crate) struct SearchResult {
    /// The matching content, optionally with Meilisearch highlight markers.
    pub content: String,
    /// File path (for file results).
    pub file_path: Option<String>,
    /// Chunk type label (function, struct, class, etc.) — file results only.
    pub chunk_type: Option<String>,
    /// Chunk name (function name, struct name, etc.) — file results only.
    pub chunk_name: Option<String>,
    /// 1-based start line in the original file.
    pub line_start: Option<u32>,
    /// 1-based end line in the original file.
    pub line_end: Option<u32>,
    /// File extension — file results only.
    pub extension: Option<String>,
    /// Log entry ID — log results only.
    pub log_id: Option<String>,
    /// ISO 8601 datetime string — log results only.
    pub datetime: Option<String>,
    /// Importance level — log results only.
    pub importance: Option<String>,
    /// Meilisearch ranking score (0.0–1.0), when `showRankingScore` is enabled.
    pub ranking_score: Option<f64>,
}

// -- Configuration constants -------------------------------------------------

// The indexability gates, extension allowlist, size cap and exclusion lists
// live in the sibling `filters` module; re-exported here so existing
// `types::is_indexable` / `types::MAX_FILE_SIZE` call-sites keep resolving.
pub(crate) use crate::index::filters::{FALLBACK_CHUNK_SIZE, is_excluded_dir, is_indexable};

/// Meilisearch settings for the **files** index.
///
/// Defines which fields are searchable, filterable, and sortable.
/// See design doc §4 "Files Index" for rationale.
pub(crate) fn files_index_settings() -> serde_json::Value {
    serde_json::json!({
        "searchableAttributes": ["content", "chunk_name", "file_path"],
        "filterableAttributes": ["file_path", "extension", "chunk_type"],
        "sortableAttributes": ["last_modified_ms"],
        "typoTolerance": {
            "enabled": true,
            "minWordSizeForTypos": { "oneTypo": 4, "twoTypos": 8 }
        }
    })
}

/// Meilisearch settings for the **logs** index.
///
/// Defines which fields are searchable, filterable, and sortable.
/// See design doc §4 "Logs Index" for rationale.
pub(crate) fn logs_index_settings() -> serde_json::Value {
    serde_json::json!({
        "searchableAttributes": ["content"],
        "filterableAttributes": ["timestamp_ms", "importance", "worker_id"],
        "sortableAttributes": ["timestamp_ms"]
    })
}
