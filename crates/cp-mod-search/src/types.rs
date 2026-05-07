//! Core types for the search module.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};

use serde::{Deserialize, Serialize};

/// Persisted search state — survives TUI reloads.
///
/// Serialized via `save_module_data` / `load_module_data`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SearchPersistData {
    /// TCP port the Meilisearch server is listening on.
    pub port: u16,
    /// API master key for authenticating with Meilisearch.
    pub master_key: String,
    /// 8-character hash of the project root path (for per-project index naming).
    pub project_hash: String,
    /// Whether the initial full-project indexing has completed.
    pub index_ready: bool,
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
}

impl std::fmt::Debug for SearchState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchState")
            .field("persist", &self.persist)
            .field("indexer_tx", &self.indexer_tx.is_some())
            .field("watcher", &self.watcher.is_some())
            .field("metrics", &self.metrics)
            .finish()
    }
}

/// Newtype wrapper around `notify::RecommendedWatcher`.
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
    /// OCR: files where conversion was attempted.
    pub ocr_attempted: u64,
    /// OCR: files successfully converted to text.
    pub ocr_succeeded: u64,
    /// OCR: files that failed conversion.
    pub ocr_failed: u64,
    /// OCR: files served from the disk cache.
    pub ocr_cached: u64,
}

/// Commands sent to the background indexer thread.
#[derive(Debug)]
pub(crate) enum IndexerCmd {
    /// Read, chunk, and index a file.
    IndexFile(PathBuf),
    /// Remove all indexed chunks for a file.
    DeleteFile(PathBuf),
}

// -- Public overlay info -----------------------------------------------------

/// Information exposed to the main binary for the Ctrl+I overlay.
///
/// Constructed by [`crate::overlay_info`] from internal state.
#[derive(Debug, Clone)]
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
    /// OCR: files where conversion was attempted.
    pub ocr_attempted: u64,
    /// OCR: files successfully converted to text.
    pub ocr_succeeded: u64,
    /// OCR: files that failed conversion.
    pub ocr_failed: u64,
    /// OCR: files served from the disk cache.
    pub ocr_cached: u64,
    /// Whether the OCR API key is configured.
    pub ocr_available: bool,
}

// -- Search results ----------------------------------------------------------

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
    /// Tags — log results only.
    pub tags: Option<Vec<String>>,
}

// -- Configuration constants -------------------------------------------------

/// Maximum file size in bytes (1 MB).
///
/// Files larger than this are skipped during indexing to avoid
/// overwhelming the search index with very large generated files.
pub(crate) const MAX_FILE_SIZE: u64 = 0x0010_0000;

/// Default chunk size in characters for the fixed-size fallback splitter.
pub(crate) const FALLBACK_CHUNK_SIZE: usize = 4000;

/// Extensions that are eligible for indexing (code, config, docs, web, build).
///
/// Returns `true` if the extension is in the hardcoded allowlist.
pub(crate) fn is_allowed_extension(ext: &str) -> bool {
    matches!(
        ext,
        // Code
        "rs" | "py" | "js" | "ts" | "jsx" | "tsx"
            | "go" | "java" | "c" | "h" | "cpp" | "hpp" | "cc"
            | "rb" | "php" | "swift" | "kt" | "scala"
            | "ex" | "exs" | "hs" | "ml" | "lua" | "dart"
            | "zig" | "nix" | "tf" | "sh" | "bash" | "zsh"
            | "sql" | "cs" | "fs" | "vb" | "pl" | "pm"
            | "r" | "jl" | "nim" | "sol" | "v" | "vy" | "move"
        // Config / data
        | "toml" | "yaml" | "yml" | "json" | "xml"
            | "ini" | "cfg" | "conf" | "properties"
        // Documentation
        | "md" | "txt" | "rst" | "adoc" | "org" | "tex"
        // Web
        | "html" | "htm" | "css" | "scss" | "sass" | "less" | "svg"
        // Build
        | "dockerfile" | "makefile" | "cmake" | "gradle" | "sbt"
        // Other
        | "graphql" | "proto" | "thrift"
    )
}

/// Directory names that are always skipped during indexing.
const EXCLUDED_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "vendor",
    "target",
    "dist",
    "build",
    "out",
    "__pycache__",
    ".next",
    ".nuxt",
    ".context-pilot",
];

/// File patterns (suffixes) that are always skipped during indexing.
const EXCLUDED_SUFFIXES: &[&str] = &[".min.js", ".min.css", ".map", ".lock", ".sum"];

/// Check if a path component is an excluded directory.
pub(crate) fn is_excluded_dir(name: &str) -> bool {
    EXCLUDED_DIRS.contains(&name)
}

/// Check if a filename matches an excluded suffix pattern.
pub(crate) fn is_excluded_file(filename: &str) -> bool {
    EXCLUDED_SUFFIXES.iter().any(|suffix| filename.ends_with(suffix))
}

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
        "searchableAttributes": ["content", "tags"],
        "filterableAttributes": ["timestamp_ms", "importance", "tags", "worker_id"],
        "sortableAttributes": ["timestamp_ms"]
    })
}
