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
            CacheRequest::RefreshTmux { context_id, pane_id, current_content_hash } => {
                refresh_tmux_cache(context_id, pane_id, current_content_hash, tx);
            }
            CacheRequest::RefreshGitStatus { show_diffs, current_hash } => {
                refresh_git_status(show_diffs, current_hash, tx);
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
    current_content_hash: Option<String>,
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
    // Hash full content to detect any changes, not just last lines
    let new_hash = hash_content(&content);

    // Only send update if content actually changed
    if current_content_hash.as_ref() != Some(&new_hash) {
        let token_count = estimate_tokens(&content);
        let _ = tx.send(CacheUpdate::TmuxContent {
            context_id,
            content,
            content_hash: new_hash,
            token_count,
        });
    }
}

fn refresh_git_status(show_diffs: bool, current_hash: Option<String>, tx: Sender<CacheUpdate>) {
    let _guard = crate::profile!("cache::git_status");
    use std::process::Command;
    use std::collections::HashMap;
    use crate::state::GitChangeType;

    // Check if we're in a git repo (fast check)
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
            branches: vec![],
            formatted_content: "Not a git repository".to_string(),
            token_count: estimate_tokens("Not a git repository"),
            status_hash: String::new(),
        });
        return;
    }

    // Get status first for change detection (fast)
    let status_output = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    // Also include branch in hash (branch switch = change)
    let branch_output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let new_hash = hash_content(&format!("{}\n{}", branch_output, status_output));

    // If hash unchanged, skip expensive operations
    if current_hash.as_ref() == Some(&new_hash) {
        let _ = tx.send(CacheUpdate::GitStatusUnchanged);
        return;
    }

    // Get branch name (already have it from above)
    let branch = if branch_output == "HEAD" {
        // Detached HEAD - get short hash
        Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .map(|o| format!("detached:{}", String::from_utf8_lossy(&o.stdout).trim()))
    } else if branch_output.is_empty() {
        None
    } else {
        Some(branch_output)
    };

    // Collect per-file changes from status output (already have it)
    let mut file_changes: HashMap<String, (i32, i32, GitChangeType)> = HashMap::new();

    for line in status_output.lines() {
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

    // Only fetch numstat if we have changes (skip if working tree clean)
    if !file_changes.is_empty() {
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
                    entry.0 = lines;
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
                        entry.1 = lines;
                    }
                }
            }
        }
    }

    // Convert to vec and sort by path
    let mut changes: Vec<_> = file_changes.into_iter()
        .map(|(path, (add, del, ct))| (path, add, del, ct, String::new()))
        .collect();
    changes.sort_by(|a, b| a.0.cmp(&b.0));

    // Get all local branches
    let branches: Vec<(String, bool)> = Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            let current = branch.as_deref().unwrap_or("");
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|b| (b.to_string(), b == current))
                .collect()
        })
        .unwrap_or_default();

    // Only fetch diffs if show_diffs is enabled AND we have changes
    if show_diffs && !changes.is_empty() {
        let mut diff_contents: HashMap<String, String> = HashMap::new();

        // Get combined diff (staged + unstaged)
        if let Ok(output) = Command::new("git").args(["diff", "HEAD"]).output() {
            if output.status.success() {
                let diff_output = String::from_utf8_lossy(&output.stdout);
                parse_diff_by_file(&diff_output, &mut diff_contents);
            }
        }

        // For untracked files, create a pseudo-diff
        for (path, _, _, ct, _) in &changes {
            if *ct == GitChangeType::Added && !diff_contents.contains_key(path) {
                if let Ok(content) = std::fs::read_to_string(path) {
                    let mut pseudo_diff = format!("diff --git a/{} b/{}\n", path, path);
                    pseudo_diff.push_str("new file\n");
                    pseudo_diff.push_str(&format!("--- /dev/null\n+++ b/{}\n", path));
                    pseudo_diff.push_str("@@ -0,0 +1 @@\n");
                    for line in content.lines() {
                        pseudo_diff.push_str(&format!("+{}\n", line));
                    }
                    diff_contents.insert(path.clone(), pseudo_diff);
                }
            }
        }

        // For deleted files, create a pseudo-diff
        for (path, _, _, ct, _) in &changes {
            if *ct == GitChangeType::Deleted && !diff_contents.contains_key(path) {
                if let Ok(output) = Command::new("git").args(["show", &format!("HEAD:{}", path)]).output() {
                    if output.status.success() {
                        let content = String::from_utf8_lossy(&output.stdout);
                        let mut pseudo_diff = format!("diff --git a/{} b/{}\n", path, path);
                        pseudo_diff.push_str("deleted file\n");
                        pseudo_diff.push_str(&format!("--- a/{}\n+++ /dev/null\n", path));
                        pseudo_diff.push_str("@@ -1 +0,0 @@\n");
                        for line in content.lines() {
                            pseudo_diff.push_str(&format!("-{}\n", line));
                        }
                        diff_contents.insert(path.clone(), pseudo_diff);
                    }
                }
            }
        }

        // Attach diff content to changes
        for (path, _, _, _, diff) in &mut changes {
            if let Some(d) = diff_contents.remove(path) {
                *diff = d;
            }
        }
    }

    // Generate formatted content for LLM context
    let formatted_content = format_git_content(&branch, &changes, show_diffs);
    let token_count = estimate_tokens(&formatted_content);

    let _ = tx.send(CacheUpdate::GitStatus {
        branch,
        is_repo: true,
        file_changes: changes,
        branches,
        formatted_content,
        token_count,
        status_hash: new_hash,
    });
}

/// Format git status for LLM context (as markdown table + optional diffs)
fn format_git_content(
    branch: &Option<String>,
    changes: &[(String, i32, i32, crate::state::GitChangeType, String)],
    show_diffs: bool,
) -> String {
    use crate::state::GitChangeType;

    let mut output = String::new();

    // Branch
    if let Some(branch) = branch {
        output.push_str(&format!("Branch: {}\n", branch));
    }

    if changes.is_empty() {
        output.push_str("\nWorking tree clean\n");
    } else {
        output.push_str("\n| File | Type | + | - | Net |\n");
        output.push_str("|------|------|---|---|-----|\n");

        let mut total_add: i32 = 0;
        let mut total_del: i32 = 0;

        for (path, additions, deletions, change_type, _) in changes {
            total_add += additions;
            total_del += deletions;
            let net = additions - deletions;
            let net_str = if net >= 0 { format!("+{}", net) } else { format!("{}", net) };
            let type_str = match change_type {
                GitChangeType::Added => "A",
                GitChangeType::Deleted => "D",
                GitChangeType::Modified => "M",
                GitChangeType::Renamed => "R",
            };
            output.push_str(&format!("| {} | {} | +{} | -{} | {} |\n",
                path, type_str, additions, deletions, net_str));
        }

        let total_net = total_add - total_del;
        let total_net_str = if total_net >= 0 { format!("+{}", total_net) } else { format!("{}", total_net) };
        output.push_str(&format!("| **Total** | | **+{}** | **-{}** | **{}** |\n",
            total_add, total_del, total_net_str));

        // Add diff content only if show_diffs is enabled
        if show_diffs {
            output.push_str("\n## Diffs\n\n");
            for (_, _, _, _, diff_content) in changes {
                if !diff_content.is_empty() {
                    output.push_str("```diff\n");
                    output.push_str(diff_content);
                    output.push_str("```\n\n");
                }
            }
        }
    }

    output
}

/// Parse unified diff output and group by file
fn parse_diff_by_file(diff_output: &str, diff_contents: &mut std::collections::HashMap<String, String>) {
    let mut current_file: Option<String> = None;
    let mut current_diff = String::new();

    for line in diff_output.lines() {
        if line.starts_with("diff --git") {
            // Save previous file's diff
            if let Some(file) = current_file.take() {
                if !current_diff.is_empty() {
                    diff_contents.insert(file, current_diff.clone());
                }
            }
            current_diff.clear();

            // Extract file path from "diff --git a/path b/path"
            if let Some(b_part) = line.split(" b/").nth(1) {
                current_file = Some(b_part.to_string());
            }
            current_diff.push_str(line);
            current_diff.push('\n');
        } else if current_file.is_some() {
            current_diff.push_str(line);
            current_diff.push('\n');
        }
    }

    // Save last file's diff
    if let Some(file) = current_file {
        if !current_diff.is_empty() {
            diff_contents.insert(file, current_diff);
        }
    }
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
