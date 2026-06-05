//! Search index overlay IR types (Ctrl+I).
//!
//! Split from `conversation.rs` to keep it under the 500-line limit.
//! These types model the Meilisearch indexing status overlay.

use serde::{Deserialize, Serialize};

use crate::Semantic;

/// Meilisearch indexing status overlay (Ctrl+I).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchIndexOverlay {
    /// Title text (may include "✓ Copied!" flash).
    pub title: String,
    /// Footer hint text.
    pub footer: String,
    /// Server status section.
    pub server: SearchServer,
    /// Core index statistics.
    pub index: SearchIndex,
    /// Extension breakdown (bar chart).
    pub extensions: Vec<SearchExtension>,
    /// Splitter statistics (tree-sitter vs fallback).
    pub splitter: Option<SearchSplitter>,
    /// Embedding statistics.
    pub embeddings: Option<SearchEmbeddings>,
    /// Recent Meilisearch tasks.
    pub recent_tasks: Vec<SearchTask>,
    /// Top recomputed files.
    pub top_recomputed: Vec<SearchRecomputed>,
    /// Recently sent files.
    pub recently_sent: Vec<SearchRecentFile>,
}

/// Server status for the search index overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchServer {
    /// Server URL (e.g. `http://127.0.0.1:49166`).
    pub url: String,
    /// Whether the server is online.
    pub online: bool,
    /// Meilisearch version string.
    pub version: String,
    /// CPU usage percentage (if available).
    pub cpu_pct: Option<f64>,
    /// Semantic colour for CPU usage.
    pub cpu_semantic: Semantic,
    /// Memory display string (e.g. "215 MB"), if available.
    pub memory_display: Option<String>,
}

/// Core index statistics for the search overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchIndex {
    /// Number of files indexed.
    pub files_indexed: u64,
    /// Number of chunks indexed.
    pub chunks_indexed: u64,
    /// Pending queue depth.
    pub queue_depth: u64,
    /// Error count.
    pub error_count: u64,
    /// Whether the index is ready (vs scanning).
    pub index_ready: bool,
    /// Last activity as relative string (e.g. "5s ago").
    pub last_activity: String,
    /// Used disk space display (e.g. "12 MB").
    pub disk_used: String,
    /// Total disk space display (e.g. "45 MB").
    pub disk_total: String,
    /// Document store size display (e.g. "8 MB").
    pub docs_display: String,
    /// Average chunk size display, if available.
    pub avg_chunk: Option<String>,
}

/// Extension entry for the bar chart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchExtension {
    /// Extension name (e.g. "rs", "ts").
    pub name: String,
    /// File count.
    pub count: u64,
    /// Pre-computed bar width (character cells).
    pub bar_width: usize,
    /// Percentage of total files.
    pub pct: u64,
}

/// Splitter statistics (tree-sitter vs fixed-size fallback).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SearchSplitter {
    /// Tree-sitter chunk count.
    pub tree_sitter_chunks: u64,
    /// Tree-sitter percentage.
    pub tree_sitter_pct: u64,
    /// Fallback chunk count.
    pub fallback_chunks: u64,
    /// Fallback percentage.
    pub fallback_pct: u64,
}

/// Embedding statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchEmbeddings {
    /// Model name (e.g. "voyage-code-3").
    pub model: String,
    /// Number of embedding vectors.
    pub vector_count: u64,
    /// Whether embeddings are currently being generated.
    pub is_indexing: bool,
    /// Number of embedded documents.
    pub embedded_docs: u64,
    /// Total number of documents.
    pub total_docs: u64,
    /// Coverage percentage.
    pub coverage_pct: u64,
    /// Semantic colour for coverage.
    pub coverage_semantic: Semantic,
    /// Number of log documents.
    pub logs_doc_count: u64,
}

/// A recent Meilisearch task entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchTask {
    /// Task UID.
    pub uid: u64,
    /// Task type (e.g. "indexAddition").
    pub task_type: String,
    /// Status string (e.g. "succeeded", "failed").
    pub status: String,
    /// Semantic colour for the status.
    pub status_semantic: Semantic,
    /// Duration display string.
    pub duration: String,
}

/// A recomputed file entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRecomputed {
    /// File path (may be truncated).
    pub path: String,
    /// Recompute count.
    pub count: u64,
}

/// A recently sent file entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRecentFile {
    /// File path (may be truncated).
    pub path: String,
    /// Relative time ago string.
    pub ago: String,
}
