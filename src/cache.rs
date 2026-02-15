//! Background cache manager for non-blocking cache operations.
//!
//! This module handles cache invalidation and seeding in background threads
//! to ensure the main UI thread is never blocked.

use std::sync::mpsc::{self, Sender};
use std::thread;

use sha2::{Digest, Sha256};

use crate::state::TreeFileDescription;

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
        file_changes: Vec<(String, i32, i32, crate::state::GitChangeType, String)>,
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
#[allow(clippy::enum_variant_names)]
pub enum CacheRequest {
    /// Refresh a file's cache
    RefreshFile { context_id: String, file_path: String, current_source_hash: Option<String> },
    /// Refresh tree cache
    RefreshTree {
        context_id: String,
        tree_filter: String,
        tree_open_folders: Vec<String>,
        tree_descriptions: Vec<TreeFileDescription>,
    },
    /// Refresh glob cache
    RefreshGlob { context_id: String, pattern: String, base_path: Option<String> },
    /// Refresh grep cache
    RefreshGrep { context_id: String, pattern: String, path: Option<String>, file_pattern: Option<String> },
    /// Refresh tmux pane cache
    RefreshTmux { context_id: String, pane_id: String, lines: Option<usize>, current_source_hash: Option<String> },
    /// Refresh git status
    RefreshGitStatus {
        /// Whether to include full diff content in formatted output
        show_diffs: bool,
        /// Current source hash (for change detection - skip if unchanged)
        current_source_hash: Option<String>,
        /// Diff base ref (e.g., "HEAD~3", "main") — None means default (HEAD/working tree)
        diff_base: Option<String>,
    },
    /// Refresh a git result panel (re-execute read-only git command)
    RefreshGitResult { context_id: String, command: String },
    /// Refresh a GitHub result panel (re-execute read-only gh command)
    RefreshGithubResult { context_id: String, command: String, github_token: String },
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
            CacheRequest::RefreshGitResult { .. } => ContextType::GitResult,
            CacheRequest::RefreshGithubResult { .. } => ContextType::GithubResult,
        }
    }
}

/// Hash content for change detection (SHA-256, collision-resistant)
pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:064x}", hasher.finalize())
}

/// Maximum concurrent cache worker threads
const CACHE_POOL_SIZE: usize = 6;

/// Bounded thread pool for cache operations.
/// Workers pull (CacheRequest, Sender<CacheUpdate>) pairs from a shared channel.
pub struct CachePool {
    job_tx: Sender<(CacheRequest, Sender<CacheUpdate>)>,
}

impl CachePool {
    /// Create a new pool with CACHE_POOL_SIZE worker threads.
    pub fn new() -> Self {
        let (job_tx, job_rx) = mpsc::channel::<(CacheRequest, Sender<CacheUpdate>)>();
        let job_rx = std::sync::Arc::new(std::sync::Mutex::new(job_rx));

        for i in 0..CACHE_POOL_SIZE {
            let rx = std::sync::Arc::clone(&job_rx);
            thread::Builder::new()
                .name(format!("cache-worker-{}", i))
                .spawn(move || {
                    loop {
                        let job = {
                            let lock = rx.lock().unwrap_or_else(|e| e.into_inner());
                            lock.recv()
                        };
                        match job {
                            Ok((request, tx)) => {
                                let context_type = request.context_type();
                                if let Some(panel) = crate::modules::create_panel(context_type)
                                    && let Some(update) = panel.refresh_cache(request)
                                {
                                    let _ = tx.send(update);
                                }
                            }
                            Err(_) => break, // Channel closed, pool shutting down
                        }
                    }
                })
                .ok(); // If thread spawn fails, pool just has fewer workers
        }

        Self { job_tx }
    }

    /// Submit a cache request to the pool.
    pub fn submit(&self, request: CacheRequest, tx: Sender<CacheUpdate>) {
        let _ = self.job_tx.send((request, tx));
    }
}

/// Global cache pool instance
static CACHE_POOL: std::sync::LazyLock<CachePool> = std::sync::LazyLock::new(CachePool::new);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_content_empty_deterministic() {
        let h = hash_content("");
        // SHA-256 of empty string is well-known
        assert_eq!(h, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn hash_content_abc() {
        let h = hash_content("abc");
        assert_eq!(h, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn hash_content_different_inputs() {
        assert_ne!(hash_content("hello"), hash_content("world"));
    }

    #[test]
    fn hash_content_idempotent() {
        assert_eq!(hash_content("test"), hash_content("test"));
    }

    #[test]
    fn hash_content_length_64() {
        // SHA-256 hex is always 64 chars
        assert_eq!(hash_content("anything").len(), 64);
    }
}

/// Process a cache request in the background via the bounded thread pool.
pub fn process_cache_request(request: CacheRequest, tx: Sender<CacheUpdate>) {
    CACHE_POOL.submit(request, tx);
}
