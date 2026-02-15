use sha2::{Digest, Sha256};

use crate::state::ContextType;
use crate::types::git::GitChangeType;
use crate::types::tree::TreeFileDescription;

/// Result of a background cache operation
#[derive(Debug, Clone)]
pub enum CacheUpdate {
    /// Generic content update (used by File, Tree, Glob, Grep, Tmux, GitResult, GithubResult)
    Content { context_id: String, content: String, token_count: usize },
    /// Git status was fetched (special case: writes to State fields, not just ContextElement)
    GitStatus {
        branch: Option<String>,
        is_repo: bool,
        /// (path, additions, deletions, change_type, diff_content)
        file_changes: Vec<(String, i32, i32, GitChangeType, String)>,
        /// All local branches (name, is_current)
        branches: Vec<(String, bool)>,
        /// Formatted content for LLM context
        formatted_content: String,
        /// Token count for formatted content
        token_count: usize,
        /// Source data hash for early-exit optimization on next refresh
        source_hash: String,
    },
    /// Git status unchanged (hash matched, no need to update)
    GitStatusUnchanged,
    /// Content unchanged — clear cache_in_flight without updating content
    Unchanged { context_id: String },
}

/// Request for background cache operations
#[derive(Debug, Clone)]
pub enum CacheRequest {
    /// Refresh a file's cache
    File { context_id: String, file_path: String, current_source_hash: Option<String> },
    /// Refresh tree cache
    Tree {
        context_id: String,
        tree_filter: String,
        tree_open_folders: Vec<String>,
        tree_descriptions: Vec<TreeFileDescription>,
    },
    /// Refresh glob cache
    Glob { context_id: String, pattern: String, base_path: Option<String> },
    /// Refresh grep cache
    Grep { context_id: String, pattern: String, path: Option<String>, file_pattern: Option<String> },
    /// Refresh tmux pane cache
    Tmux { context_id: String, pane_id: String, lines: Option<usize>, current_source_hash: Option<String> },
    /// Refresh git status
    GitStatus {
        /// Whether to include full diff content in formatted output
        show_diffs: bool,
        /// Current source hash (for change detection - skip if unchanged)
        current_source_hash: Option<String>,
        /// Diff base ref (e.g., "HEAD~3", "main") — None means default (HEAD/working tree)
        diff_base: Option<String>,
    },
    /// Refresh a git result panel (re-execute read-only git command)
    GitResult { context_id: String, command: String },
    /// Refresh a GitHub result panel (re-execute read-only gh command)
    GithubResult { context_id: String, command: String, github_token: String },
}

impl CacheRequest {
    /// Get the context type this request is for, to dispatch to the correct panel.
    pub fn context_type(&self) -> ContextType {
        match self {
            CacheRequest::File { .. } => ContextType::new(ContextType::FILE),
            CacheRequest::Tree { .. } => ContextType::new(ContextType::TREE),
            CacheRequest::Glob { .. } => ContextType::new(ContextType::GLOB),
            CacheRequest::Grep { .. } => ContextType::new(ContextType::GREP),
            CacheRequest::Tmux { .. } => ContextType::new(ContextType::TMUX),
            CacheRequest::GitStatus { .. } => ContextType::new(ContextType::GIT),
            CacheRequest::GitResult { .. } => ContextType::new(ContextType::GIT_RESULT),
            CacheRequest::GithubResult { .. } => ContextType::new(ContextType::GITHUB_RESULT),
        }
    }
}

/// Hash content for change detection (SHA-256, collision-resistant)
pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:064x}", hasher.finalize())
}
