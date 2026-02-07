//! Background cache manager for non-blocking cache operations.
//!
//! This module handles cache invalidation and seeding in background threads
//! to ensure the main UI thread is never blocked.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::mpsc::Sender;
use std::thread;

use crate::state::TreeFileDescription;

/// Result of a background cache operation
#[derive(Debug, Clone)]
pub enum CacheUpdate {
    /// File content was read
    FileContent {
        context_id: String,
        content: String,
        hash: String,
        token_count: usize,
    },
    /// Tree content was generated
    TreeContent {
        context_id: String,
        content: String,
        token_count: usize,
    },
    /// Glob results were computed
    GlobContent {
        context_id: String,
        content: String,
        token_count: usize,
    },
    /// Grep results were computed
    GrepContent {
        context_id: String,
        content: String,
        token_count: usize,
    },
    /// Tmux pane content was captured
    TmuxContent {
        context_id: String,
        content: String,
        content_hash: String,
        token_count: usize,
    },
    /// Git status was fetched
    GitStatus {
        branch: Option<String>,
        is_repo: bool,
        /// (path, additions, deletions, change_type, diff_content)
        file_changes: Vec<(String, i32, i32, crate::state::GitChangeType, String)>,
        /// All local branches (name, is_current)
        branches: Vec<(String, bool)>,
        /// Formatted content for LLM context
        formatted_content: String,
        /// Token count for formatted content
        token_count: usize,
        /// Hash of git status --porcelain output (for change detection)
        status_hash: String,
    },
    /// Git status unchanged (hash matched, no need to update)
    GitStatusUnchanged,
}

/// Request for background cache operations
#[derive(Debug, Clone)]
pub enum CacheRequest {
    /// Refresh a file's cache
    RefreshFile {
        context_id: String,
        file_path: String,
        current_hash: Option<String>,
    },
    /// Refresh tree cache
    RefreshTree {
        context_id: String,
        tree_filter: String,
        tree_open_folders: Vec<String>,
        tree_descriptions: Vec<TreeFileDescription>,
    },
    /// Refresh glob cache
    RefreshGlob {
        context_id: String,
        pattern: String,
        base_path: Option<String>,
    },
    /// Refresh grep cache
    RefreshGrep {
        context_id: String,
        pattern: String,
        path: Option<String>,
        file_pattern: Option<String>,
    },
    /// Refresh tmux pane cache
    RefreshTmux {
        context_id: String,
        pane_id: String,
        current_content_hash: Option<String>,
    },
    /// Refresh git status
    RefreshGitStatus {
        /// Whether to include full diff content in formatted output
        show_diffs: bool,
        /// Current status hash (for change detection - skip if unchanged)
        current_hash: Option<String>,
    },
}

impl CacheRequest {
    /// Get the context type this request is for, to dispatch to the correct panel.
    pub fn context_type(&self) -> crate::state::ContextType {
        use crate::state::ContextType;
        match self {
            CacheRequest::RefreshFile { .. } => ContextType::File,
            CacheRequest::RefreshTree { .. } => ContextType::Tree,
            CacheRequest::RefreshGlob { .. } => ContextType::Glob,
            CacheRequest::RefreshGrep { .. } => ContextType::Grep,
            CacheRequest::RefreshTmux { .. } => ContextType::Tmux,
            CacheRequest::RefreshGitStatus { .. } => ContextType::Git,
        }
    }
}

/// Hash content for change detection
pub fn hash_content(content: &str) -> String {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Process a cache request in the background by dispatching to the appropriate panel.
pub fn process_cache_request(request: CacheRequest, tx: Sender<CacheUpdate>) {
    thread::spawn(move || {
        let context_type = request.context_type();
        if let Some(panel) = crate::modules::create_panel(context_type) {
            if let Some(update) = panel.refresh_cache(request) {
                let _ = tx.send(update);
            }
        }
    });
}
