//! Background cache manager for non-blocking cache operations.
//!
//! This module handles cache invalidation and seeding in background threads
//! to ensure the main UI thread is never blocked.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;

use crate::state::{estimate_tokens, TreeFileDescription};

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
        last_lines_hash: String,
        token_count: usize,
    },
    /// Git status was fetched
    GitStatus {
        branch: Option<String>,
        is_repo: bool,
        file_changes: Vec<(String, i32, i32, crate::state::GitChangeType)>,
    },
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
        current_last_lines_hash: Option<String>,
    },
    /// Refresh git status
    RefreshGitStatus,
}

/// Hash content for change detection
pub fn hash_content(content: &str) -> String {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Get the last N lines of content and hash them
pub fn hash_last_lines(content: &str, n: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(n);
    let last_lines = lines[start..].join("\n");
    hash_content(&last_lines)
}

/// Process a cache request in the background
pub fn process_cache_request(request: CacheRequest, tx: Sender<CacheUpdate>) {
    thread::spawn(move || {
        match request {
            CacheRequest::RefreshFile { context_id, file_path, current_hash } => {
                refresh_file_cache(context_id, file_path, current_hash, tx);
            }
            CacheRequest::RefreshTree { context_id, tree_filter, tree_open_folders, tree_descriptions } => {
                refresh_tree_cache(context_id, tree_filter, tree_open_folders, tree_descriptions, tx);
            }
            CacheRequest::RefreshGlob { context_id, pattern, base_path } => {
                refresh_glob_cache(context_id, pattern, base_path, tx);
            }
            CacheRequest::RefreshGrep { context_id, pattern, path, file_pattern } => {
                refresh_grep_cache(context_id, pattern, path, file_pattern, tx);
            }
            CacheRequest::RefreshTmux { context_id, pane_id, current_last_lines_hash } => {
                refresh_tmux_cache(context_id, pane_id, current_last_lines_hash, tx);
            }
            CacheRequest::RefreshGitStatus => {
                refresh_git_status(tx);
            }
        }
    });
}

fn refresh_file_cache(
    context_id: String,
    file_path: String,
    current_hash: Option<String>,
    tx: Sender<CacheUpdate>,
) {
    let path = PathBuf::from(&file_path);
    if !path.exists() {
        return;
    }

    let Ok(content) = fs::read_to_string(&path) else {
        return;
    };

    let new_hash = hash_content(&content);

    // Only send update if hash changed or no current hash
    if current_hash.as_ref() != Some(&new_hash) {
        let token_count = estimate_tokens(&content);
        let _ = tx.send(CacheUpdate::FileContent {
            context_id,
            content,
            hash: new_hash,
            token_count,
        });
    }
}

fn refresh_tree_cache(
    context_id: String,
    tree_filter: String,
    tree_open_folders: Vec<String>,
    tree_descriptions: Vec<TreeFileDescription>,
    tx: Sender<CacheUpdate>,
) {
    use crate::tools::tree::generate_tree_string;

    let content = generate_tree_string(&tree_filter, &tree_open_folders, &tree_descriptions);
    let token_count = estimate_tokens(&content);

    let _ = tx.send(CacheUpdate::TreeContent {
        context_id,
        content,
        token_count,
    });
}

fn refresh_glob_cache(
    context_id: String,
    pattern: String,
    base_path: Option<String>,
    tx: Sender<CacheUpdate>,
) {
    use crate::tools::compute_glob_results;

    let base = base_path.as_deref().unwrap_or(".");
    let (content, _count) = compute_glob_results(&pattern, base);
    let token_count = estimate_tokens(&content);

    let _ = tx.send(CacheUpdate::GlobContent {
        context_id,
        content: content.to_string(),
        token_count,
    });
}

fn refresh_grep_cache(
    context_id: String,
    pattern: String,
    path: Option<String>,
    file_pattern: Option<String>,
    tx: Sender<CacheUpdate>,
) {
    use crate::tools::compute_grep_results;

    let search_path = path.as_deref().unwrap_or(".");
    let (content, _count) = compute_grep_results(&pattern, search_path, file_pattern.as_deref());
    let token_count = estimate_tokens(&content);

    let _ = tx.send(CacheUpdate::GrepContent {
        context_id,
        content: content.to_string(),
        token_count,
    });
}

fn refresh_tmux_cache(
    context_id: String,
    pane_id: String,
    current_last_lines_hash: Option<String>,
    tx: Sender<CacheUpdate>,
) {
    use std::process::Command;

    // Capture tmux pane content
    let output = Command::new("tmux")
        .args(["capture-pane", "-p", "-t", &pane_id])
        .output();

    let Ok(output) = output else {
        return;
    };

    if !output.status.success() {
        return;
    }

    let content = String::from_utf8_lossy(&output.stdout).to_string();
    let new_hash = hash_last_lines(&content, 2);

    // Only send update if last lines changed
    if current_last_lines_hash.as_ref() != Some(&new_hash) {
        let token_count = estimate_tokens(&content);
        let _ = tx.send(CacheUpdate::TmuxContent {
            context_id,
            content,
            last_lines_hash: new_hash,
            token_count,
        });
    }
}

fn refresh_git_status(tx: Sender<CacheUpdate>) {
    use std::process::Command;
    use std::collections::HashMap;
    use crate::state::GitChangeType;

    // Check if we're in a git repo
    let is_repo = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !is_repo {
        let _ = tx.send(CacheUpdate::GitStatus {
            branch: None,
            is_repo: false,
            file_changes: vec![],
        });
        return;
    }

    // Get branch name
    let branch = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let name = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if name == "HEAD" {
                    // Detached HEAD - get short hash
                    Command::new("git")
                        .args(["rev-parse", "--short", "HEAD"])
                        .output()
                        .ok()
                        .map(|o| format!("detached:{}", String::from_utf8_lossy(&o.stdout).trim()))
                } else {
                    Some(name)
                }
            } else {
                None
            }
        });

    // Collect per-file changes
    // Map: path -> (additions, deletions, change_type)
    let mut file_changes: HashMap<String, (i32, i32, GitChangeType)> = HashMap::new();

    // Get status to identify file types (added, deleted, modified, untracked)
    if let Ok(output) = Command::new("git").args(["status", "--porcelain"]).output() {
        if output.status.success() {
            let content = String::from_utf8_lossy(&output.stdout);
            for line in content.lines() {
                if line.len() < 3 {
                    continue;
                }
                let x = line.chars().next().unwrap_or(' ');
                let y = line.chars().nth(1).unwrap_or(' ');
                let path = line[3..].trim().to_string();
                // Handle renames: "R  old -> new"
                let path = if path.contains(" -> ") {
                    path.split(" -> ").last().unwrap_or(&path).to_string()
                } else {
                    path
                };

                let change_type = match (x, y) {
                    ('?', '?') => GitChangeType::Added, // Untracked = new file
                    ('A', _) | (_, 'A') => GitChangeType::Added,
                    ('D', _) | (_, 'D') => GitChangeType::Deleted,
                    ('R', _) | (_, 'R') => GitChangeType::Renamed,
                    _ => GitChangeType::Modified,
                };

                file_changes.entry(path).or_insert((0, 0, change_type));
            }
        }
    }

    // Get line counts for staged changes
    if let Ok(output) = Command::new("git").args(["diff", "--cached", "--numstat"]).output() {
        if output.status.success() {
            parse_numstat_to_map(&String::from_utf8_lossy(&output.stdout), &mut file_changes);
        }
    }

    // Get line counts for unstaged changes
    if let Ok(output) = Command::new("git").args(["diff", "--numstat"]).output() {
        if output.status.success() {
            parse_numstat_to_map(&String::from_utf8_lossy(&output.stdout), &mut file_changes);
        }
    }

    // For untracked files, count lines
    let untracked_files: Vec<String> = file_changes.iter()
        .filter(|(_, (add, del, ct))| *ct == GitChangeType::Added && *add == 0 && *del == 0)
        .map(|(path, _)| path.clone())
        .collect();

    for path in untracked_files {
        if let Ok(content) = std::fs::read_to_string(&path) {
            let lines = content.lines().count() as i32;
            if let Some(entry) = file_changes.get_mut(&path) {
                entry.0 = lines; // additions = total lines for new files
            }
        }
    }

    // For deleted files, get line count from HEAD
    let deleted_files: Vec<String> = file_changes.iter()
        .filter(|(_, (add, del, ct))| *ct == GitChangeType::Deleted && *add == 0 && *del == 0)
        .map(|(path, _)| path.clone())
        .collect();

    for path in deleted_files {
        if let Ok(output) = Command::new("git").args(["show", &format!("HEAD:{}", path)]).output() {
            if output.status.success() {
                let content = String::from_utf8_lossy(&output.stdout);
                let lines = content.lines().count() as i32;
                if let Some(entry) = file_changes.get_mut(&path) {
                    entry.1 = lines; // deletions = total lines for deleted files
                }
            }
        }
    }

    // Convert to vec and sort by path
    let mut changes: Vec<_> = file_changes.into_iter()
        .map(|(path, (add, del, ct))| (path, add, del, ct))
        .collect();
    changes.sort_by(|a, b| a.0.cmp(&b.0));

    let _ = tx.send(CacheUpdate::GitStatus {
        branch,
        is_repo: true,
        file_changes: changes,
    });
}

/// Parse git diff --numstat output and add to file_changes map
fn parse_numstat_to_map(
    output: &str,
    file_changes: &mut std::collections::HashMap<String, (i32, i32, crate::state::GitChangeType)>,
) {
    use crate::state::GitChangeType;

    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 {
            let add: i32 = parts[0].parse().unwrap_or(0);
            let del: i32 = parts[1].parse().unwrap_or(0);
            let path = parts[2].to_string();
            // Handle renames: "old => new" or "{old => new}"
            let path = if path.contains(" => ") {
                path.split(" => ").last().unwrap_or(&path).trim_end_matches('}').to_string()
            } else {
                path
            };

            let entry = file_changes.entry(path).or_insert((0, 0, GitChangeType::Modified));
            entry.0 += add;
            entry.1 += del;
        }
    }
}
