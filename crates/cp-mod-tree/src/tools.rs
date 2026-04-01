use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use ignore::gitignore::GitignoreBuilder;
use sha2::{Digest as _, Sha256};

use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::{TreeFileDescription, TreeState};
use std::fmt::Write as _;

/// Whether to show `.context-pilot/` in the tree (opt-in via env var).
static SHOW_CONTEXT_PILOT: LazyLock<bool> =
    LazyLock::new(|| std::env::var("SHOW_CONTEXT_PILOT_IN_TREE").is_ok_and(|v| v == "1" || v == "true"));

/// Mark tree context cache as deprecated (needs refresh)
fn invalidate_tree_cache(state: &mut State) {
    cp_base::panels::mark_panels_dirty(state, Kind::TREE);
}

/// Generate tree string without mutating state (for read-only rendering)
pub(crate) fn generate_tree_string(
    tree_filter: &str,
    tree_open_folders: &[String],
    tree_descriptions: &[TreeFileDescription],
) -> String {
    let root = PathBuf::from(".");

    // Build gitignore matcher from filter
    let mut builder = GitignoreBuilder::new(&root);
    for line in tree_filter.lines() {
        let line = line.trim();
        if !line.is_empty() && !line.starts_with('#') {
            let _: Option<&mut GitignoreBuilder> = builder.add_line(None, line).ok();
        }
    }
    let gitignore = builder.build().ok();

    // Build set of open folders for quick lookup
    let open_set: HashSet<_> = tree_open_folders.iter().cloned().collect();

    // Build map of descriptions for quick lookup
    let desc_map: std::collections::HashMap<_, _> = tree_descriptions.iter().map(|d| (d.path.clone(), d)).collect();

    let mut output = String::new();

    // Show pwd at the top
    if let Ok(cwd) = std::env::current_dir() {
        let _r = writeln!(output, "pwd: {}", cwd.display());
    }

    // Build tree recursively - directly show contents without root folder line
    let ctx = TreeContext { gitignore: gitignore.as_ref(), open_set: &open_set, desc_map: &desc_map };
    build_tree_new(&TreeNode { dir: &root, path_str: ".", prefix: "" }, &ctx, &mut output);

    output
}

/// Compute a short hash for a file's contents
fn compute_file_hash(path: &Path) -> Option<String> {
    let content = fs::read(path).ok()?;
    let hash = Sha256::digest(&content);
    let hex = format!("{hash:x}");
    Some(hex.get(..8).unwrap_or(&hex).to_string())
}

/// Execute `tree_toggle_folders` tool - open or close folders
pub(crate) fn execute_toggle_folders(tool: &ToolUse, state: &mut State) -> ToolResult {
    let paths = tool
        .input
        .get("paths")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();

    let action = tool.input.get("action").and_then(|v| v.as_str()).unwrap_or("toggle");

    if paths.is_empty() {
        return ToolResult::new(tool.id.clone(), "Missing 'paths' parameter".to_string(), true);
    }

    let mut opened = Vec::new();
    let mut closed = Vec::new();
    let mut errors = Vec::new();

    for path_str in paths {
        // Normalize path
        let path = PathBuf::from(path_str);
        let normalized = normalize_path(&path);

        // Verify it's a directory
        if !path.is_dir() && normalized != "." {
            errors.push(format!("{path_str}: not a directory"));
            continue;
        }

        let ts = TreeState::get(state);
        let is_open = ts.open_folders.contains(&normalized);

        match action {
            "open" => {
                if !is_open {
                    TreeState::get_mut(state).open_folders.push(normalized.clone());
                    opened.push(normalized);
                }
            }
            "close" => {
                // Don't allow closing root
                if normalized == "." {
                    errors.push("Cannot close root folder".to_string());
                    continue;
                }
                if is_open {
                    let ts_mut = TreeState::get_mut(state);
                    ts_mut.open_folders.retain(|p| p != &normalized);
                    // Also close all children
                    let prefix = format!("{normalized}/");
                    ts_mut.open_folders.retain(|p| !p.starts_with(&prefix));
                    closed.push(normalized);
                }
            }
            _ => {
                // toggle
                if is_open && normalized != "." {
                    let ts_toggle = TreeState::get_mut(state);
                    ts_toggle.open_folders.retain(|p| p != &normalized);
                    let prefix = format!("{normalized}/");
                    ts_toggle.open_folders.retain(|p| !p.starts_with(&prefix));
                    closed.push(normalized);
                } else if !is_open {
                    TreeState::get_mut(state).open_folders.push(normalized.clone());
                    opened.push(normalized);
                }
            }
        }
    }

    let mut result = Vec::new();
    if !opened.is_empty() {
        result.push(format!("Opened: {}", opened.join(", ")));
    }
    if !closed.is_empty() {
        result.push(format!("Closed: {}", closed.join(", ")));
    }
    if !errors.is_empty() {
        result.push(format!("Errors: {}", errors.join(", ")));
    }

    // Invalidate tree cache to trigger refresh
    if !opened.is_empty() || !closed.is_empty() {
        invalidate_tree_cache(state);
    }

    ToolResult::new(
        tool.id.clone(),
        if result.is_empty() { "No changes".to_string() } else { result.join("\n") },
        false,
    )
}

/// Execute `tree_describe_files` tool - add/update/remove file descriptions
pub(crate) fn execute_describe_files(tool: &ToolUse, state: &mut State) -> ToolResult {
    let descriptions = tool.input.get("descriptions").and_then(|v| v.as_array());

    let Some(descriptions) = descriptions else {
        return ToolResult::new(tool.id.clone(), "Missing 'descriptions' parameter".to_string(), true);
    };

    let mut added = Vec::new();
    let mut updated = Vec::new();
    let mut removed = Vec::new();
    let mut errors = Vec::new();

    for desc_obj in descriptions {
        let Some(path_str) = desc_obj.get("path").and_then(|v| v.as_str()) else {
            errors.push("Missing 'path' in description".to_string());
            continue;
        };

        let path = PathBuf::from(path_str);
        let normalized = normalize_path(&path);

        // Check if delete is requested
        if desc_obj.get("delete").and_then(serde_json::Value::as_bool).unwrap_or(false) {
            if TreeState::get(state).descriptions.iter().any(|d| d.path == normalized) {
                TreeState::get_mut(state).descriptions.retain(|d| d.path != normalized);
                removed.push(normalized);
            }
            continue;
        }

        let description = if let Some(d) = desc_obj.get("description").and_then(|v| v.as_str()) {
            d.to_string()
        } else {
            errors.push(format!("{path_str}: missing 'description'"));
            continue;
        };

        // Verify path exists (file or folder)
        if !path.exists() {
            errors.push(format!("{path_str}: path not found"));
            continue;
        }

        // Compute file hash
        let file_hash = compute_file_hash(&path).unwrap_or_default();

        // Update or add
        let ts = TreeState::get_mut(state);
        if let Some(existing) = ts.descriptions.iter_mut().find(|d| d.path == normalized) {
            existing.description = description;
            existing.file_hash = file_hash;
            updated.push(normalized);
        } else {
            ts.descriptions.push(TreeFileDescription { path: normalized.clone(), description, file_hash });
            added.push(normalized);
        }
    }

    let mut result = Vec::new();
    if !added.is_empty() {
        result.push(format!("Added: {}", added.join(", ")));
    }
    if !updated.is_empty() {
        result.push(format!("Updated: {}", updated.join(", ")));
    }
    if !removed.is_empty() {
        result.push(format!("Removed: {}", removed.join(", ")));
    }
    if !errors.is_empty() {
        result.push(format!("Errors: {}", errors.join("; ")));
    }

    // Invalidate tree cache to trigger refresh
    if !added.is_empty() || !updated.is_empty() || !removed.is_empty() {
        invalidate_tree_cache(state);
    }

    ToolResult::new(
        tool.id.clone(),
        if result.is_empty() { "No changes".to_string() } else { result.join("\n") },
        !errors.is_empty() && added.is_empty() && updated.is_empty() && removed.is_empty(),
    )
}

/// Execute `edit_tree_filter` tool (keep existing functionality)
pub(crate) fn execute_edit_filter(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(filter) = tool.input.get("filter").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing 'filter' parameter".to_string(), true);
    };

    TreeState::get_mut(state).filter = filter.to_string();

    // Invalidate tree cache to trigger refresh
    invalidate_tree_cache(state);

    ToolResult::new(tool.id.clone(), format!("Updated tree filter:\n{filter}"), false)
}

/// Normalize a path to a consistent format
fn normalize_path(path: &Path) -> String {
    let path_str = path.to_string_lossy();
    let normalized = path_str.trim_start_matches("./").trim_end_matches('/');

    if normalized.is_empty() || normalized == "." { ".".to_string() } else { normalized.to_string() }
}

/// List directory entries (files + folders) matching a prefix, respecting the gitignore filter.
///
/// Returns entries sorted: directories first, then alphabetically (case-insensitive).
/// Used by the `@` autocomplete popup.
#[must_use]
pub fn list_dir_entries(
    tree_filter: &str,
    dir_prefix: &str,
    name_prefix: &str,
) -> Vec<cp_base::state::autocomplete::Completion> {
    let root = PathBuf::from(".");

    // Build gitignore matcher from filter
    let mut builder = GitignoreBuilder::new(&root);
    for line in tree_filter.lines() {
        let line = line.trim();
        if !line.is_empty() && !line.starts_with('#') {
            let _: Option<&mut GitignoreBuilder> = builder.add_line(None, line).ok();
        }
    }
    let gitignore = builder.build().ok();

    let dir_path = if dir_prefix.is_empty() { PathBuf::from(".") } else { PathBuf::from(dir_prefix) };

    if !dir_path.is_dir() {
        return Vec::new();
    }

    let Ok(read) = fs::read_dir(&dir_path) else { return Vec::new() };
    let prefix_lower = name_prefix.to_lowercase();

    let mut entries: Vec<cp_base::state::autocomplete::Completion> = read
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let is_dir = path.is_dir();
            let name = entry.file_name().to_string_lossy().to_string();

            // .context-pilot/ is internal rigging — hide unless explicitly opted in
            if is_dir && name == ".context-pilot" && !*SHOW_CONTEXT_PILOT {
                return None;
            }

            // Apply gitignore filter
            if let Some(ref gi) = gitignore
                && gi.matched(&path, is_dir).is_ignore()
            {
                return None;
            }

            // Prefix match (case-insensitive)
            if !prefix_lower.is_empty() && !name.to_lowercase().starts_with(&prefix_lower) {
                return None;
            }

            Some(cp_base::state::autocomplete::Completion { name, is_dir })
        })
        .collect();

    // Sort: directories first, then alphabetically (case-insensitive)
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    entries
}

/// Context passed through tree recursion to avoid excessive parameters.
struct TreeContext<'tree> {
    /// Optional gitignore matcher for filtering entries.
    gitignore: Option<&'tree ignore::gitignore::Gitignore>,
    /// Set of folder paths currently expanded in the tree.
    open_set: &'tree HashSet<String>,
    /// Map from path to file/folder description annotation.
    desc_map: &'tree std::collections::HashMap<String, &'tree TreeFileDescription>,
}

/// Recursive traversal state for a single tree node.
struct TreeNode<'tree> {
    /// Filesystem directory path for this node.
    dir: &'tree Path,
    /// Normalized string representation of the path.
    path_str: &'tree str,
    /// Indentation prefix for tree drawing characters.
    prefix: &'tree str,
}

/// Recursively build the tree output string for a single directory node.
fn build_tree_new(node: &TreeNode<'_>, ctx: &TreeContext<'_>, output: &mut String) {
    let Ok(entries) = fs::read_dir(node.dir) else { return };

    let mut items: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|e| {
            let path = e.path();
            let is_dir = path.is_dir();
            // .context-pilot/ is internal rigging — hide unless explicitly opted in
            if is_dir && e.file_name() == ".context-pilot" && !*SHOW_CONTEXT_PILOT {
                return false;
            }
            ctx.gitignore.as_ref().is_none_or(|gi| !gi.matched(&path, is_dir).is_ignore())
        })
        .collect();

    // Sort: directories first, then alphabetically
    items.sort_by(|a, b| {
        let a_dir = a.path().is_dir();
        let b_dir = b.path().is_dir();
        match (a_dir, b_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.file_name().cmp(&b.file_name()),
        }
    });

    let total = items.len();
    for (i, entry) in items.iter().enumerate() {
        let is_last = i == total.saturating_sub(1);
        let connector = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };

        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let is_dir = entry.path().is_dir();

        // Build path string for this entry
        let entry_path =
            if node.path_str == "." { name_str.to_string() } else { format!("{}/{name_str}", node.path_str) };

        if is_dir {
            let is_open = ctx.open_set.contains(&entry_path);

            // Check for folder description
            let folder_desc = ctx.desc_map.get(&entry_path).map(|d| &d.description);

            let triangle = if is_open { "▼ " } else { "▶ " };
            if is_open {
                if let Some(desc) = folder_desc {
                    let _r = writeln!(output, "{}{connector}{triangle}{name_str}/  - {desc}", node.prefix);
                } else {
                    let _r = writeln!(output, "{}{connector}{triangle}{name_str}/", node.prefix);
                }
                let child_node = TreeNode {
                    dir: &entry.path(),
                    path_str: &entry_path,
                    prefix: &format!("{}{child_prefix}", node.prefix),
                };
                build_tree_new(&child_node, ctx, output);
            } else if let Some(desc) = folder_desc {
                let _r = writeln!(output, "{}{connector}{triangle}{name_str}/ - {desc}", node.prefix);
            } else {
                let _r = writeln!(output, "{}{connector}{triangle}{name_str}/ ", node.prefix);
            }
        } else if let Some(desc) = ctx.desc_map.get(&entry_path) {
            // Check if description is stale
            let current_hash = compute_file_hash(&entry.path()).unwrap_or_default();
            let is_stale = !desc.file_hash.is_empty() && desc.file_hash != current_hash;

            let stale_marker = if is_stale { " [!]" } else { "" };
            let _r =
                writeln!(output, "{}{}{}{} - {}", node.prefix, connector, name_str, stale_marker, desc.description);
        } else {
            let _r = writeln!(output, "{}{connector}{name_str}", node.prefix);
        }
    }
}
