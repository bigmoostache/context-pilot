use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use ignore::gitignore::GitignoreBuilder;
use sha2::{Digest, Sha256};

use cp_base::state::{ContextType, State, TreeFileDescription};
use cp_base::tools::{ToolResult, ToolUse};

/// Mark tree context cache as deprecated (needs refresh)
fn invalidate_tree_cache(state: &mut State) {
    cp_base::panels::mark_panels_dirty(state, ContextType::new(ContextType::TREE));
}

/// Generate tree string without mutating state (for read-only rendering)
pub fn generate_tree_string(
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
            let _ = builder.add_line(None, line);
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
        output.push_str(&format!("pwd: {}\n", cwd.display()));
    }

    // Build tree recursively - directly show contents without root folder line
    build_tree_new(&root, ".", "", &gitignore, &open_set, &desc_map, &mut output);

    output
}

/// Compute a short hash for a file's contents
fn compute_file_hash(path: &Path) -> Option<String> {
    let content = fs::read(path).ok()?;
    let hash = Sha256::digest(&content);
    Some(format!("{:x}", hash)[..8].to_string()) // First 8 chars
}

/// Execute tree_toggle_folders tool - open or close folders
pub fn execute_toggle_folders(tool: &ToolUse, state: &mut State) -> ToolResult {
    let paths = tool
        .input
        .get("paths")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();

    let action = tool.input.get("action").and_then(|v| v.as_str()).unwrap_or("toggle");

    if paths.is_empty() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "Missing 'paths' parameter".to_string(),
            is_error: true,
        };
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
            errors.push(format!("{}: not a directory", path_str));
            continue;
        }

        let is_open = state.tree_open_folders.contains(&normalized);

        match action {
            "open" => {
                if !is_open {
                    state.tree_open_folders.push(normalized.clone());
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
                    state.tree_open_folders.retain(|p| p != &normalized);
                    // Also close all children
                    let prefix = format!("{}/", normalized);
                    state.tree_open_folders.retain(|p| !p.starts_with(&prefix));
                    closed.push(normalized);
                }
            }
            _ => {
                // toggle
                if is_open && normalized != "." {
                    state.tree_open_folders.retain(|p| p != &normalized);
                    let prefix = format!("{}/", normalized);
                    state.tree_open_folders.retain(|p| !p.starts_with(&prefix));
                    closed.push(normalized);
                } else if !is_open {
                    state.tree_open_folders.push(normalized.clone());
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

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: if result.is_empty() { "No changes".to_string() } else { result.join("\n") },
        is_error: false,
    }
}

/// Execute tree_describe_files tool - add/update/remove file descriptions
pub fn execute_describe_files(tool: &ToolUse, state: &mut State) -> ToolResult {
    let descriptions = tool.input.get("descriptions").and_then(|v| v.as_array());

    let descriptions = match descriptions {
        Some(arr) => arr,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'descriptions' parameter".to_string(),
                is_error: true,
            };
        }
    };

    let mut added = Vec::new();
    let mut updated = Vec::new();
    let mut removed = Vec::new();
    let mut errors = Vec::new();

    for desc_obj in descriptions {
        let path_str = match desc_obj.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                errors.push("Missing 'path' in description".to_string());
                continue;
            }
        };

        let path = PathBuf::from(path_str);
        let normalized = normalize_path(&path);

        // Check if delete is requested
        if desc_obj.get("delete").and_then(|v| v.as_bool()).unwrap_or(false) {
            if state.tree_descriptions.iter().any(|d| d.path == normalized) {
                state.tree_descriptions.retain(|d| d.path != normalized);
                removed.push(normalized);
            }
            continue;
        }

        let description = match desc_obj.get("description").and_then(|v| v.as_str()) {
            Some(d) => d.to_string(),
            None => {
                errors.push(format!("{}: missing 'description'", path_str));
                continue;
            }
        };

        // Verify path exists (file or folder)
        if !path.exists() {
            errors.push(format!("{}: path not found", path_str));
            continue;
        }

        // Compute file hash
        let file_hash = compute_file_hash(&path).unwrap_or_default();

        // Update or add
        if let Some(existing) = state.tree_descriptions.iter_mut().find(|d| d.path == normalized) {
            existing.description = description;
            existing.file_hash = file_hash;
            updated.push(normalized);
        } else {
            state.tree_descriptions.push(TreeFileDescription { path: normalized.clone(), description, file_hash });
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

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: if result.is_empty() { "No changes".to_string() } else { result.join("\n") },
        is_error: !errors.is_empty() && added.is_empty() && updated.is_empty() && removed.is_empty(),
    }
}

/// Execute edit_tree_filter tool (keep existing functionality)
pub fn execute_edit_filter(tool: &ToolUse, state: &mut State) -> ToolResult {
    let filter = match tool.input.get("filter").and_then(|v| v.as_str()) {
        Some(f) => f,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'filter' parameter".to_string(),
                is_error: true,
            };
        }
    };

    state.tree_filter = filter.to_string();

    // Invalidate tree cache to trigger refresh
    invalidate_tree_cache(state);

    ToolResult { tool_use_id: tool.id.clone(), content: format!("Updated tree filter:\n{}", filter), is_error: false }
}

/// Normalize a path to a consistent format
fn normalize_path(path: &Path) -> String {
    let path_str = path.to_string_lossy();
    let normalized = path_str.trim_start_matches("./").trim_end_matches('/');

    if normalized.is_empty() || normalized == "." { ".".to_string() } else { normalized.to_string() }
}

fn build_tree_new(
    dir: &Path,
    dir_path_str: &str,
    prefix: &str,
    gitignore: &Option<ignore::gitignore::Gitignore>,
    open_set: &HashSet<String>,
    desc_map: &std::collections::HashMap<String, &TreeFileDescription>,
    output: &mut String,
) {
    let Ok(entries) = fs::read_dir(dir) else { return };

    let mut items: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            let is_dir = path.is_dir();
            if let Some(gi) = gitignore { !gi.matched(&path, is_dir).is_ignore() } else { true }
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
        let is_last = i == total - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };

        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let is_dir = entry.path().is_dir();

        // Build path string for this entry
        let entry_path =
            if dir_path_str == "." { name_str.to_string() } else { format!("{}/{}", dir_path_str, name_str) };

        if is_dir {
            let is_open = open_set.contains(&entry_path);

            // Check for folder description
            let folder_desc = desc_map.get(&entry_path).map(|d| &d.description);

            if is_open {
                if let Some(desc) = folder_desc {
                    output.push_str(&format!("{}{}{}/  - {}\n", prefix, connector, name_str, desc));
                } else {
                    output.push_str(&format!("{}{}{}/\n", prefix, connector, name_str));
                }
                build_tree_new(
                    &entry.path(),
                    &entry_path,
                    &format!("{}{}", prefix, child_prefix),
                    gitignore,
                    open_set,
                    desc_map,
                    output,
                );
            } else if let Some(desc) = folder_desc {
                output.push_str(&format!("{}{}{}/ - {}\n", prefix, connector, name_str, desc));
            } else {
                output.push_str(&format!("{}{}{}/\n", prefix, connector, name_str));
            }
        } else if let Some(desc) = desc_map.get(&entry_path) {
            // Check if description is stale
            let current_hash = compute_file_hash(&entry.path()).unwrap_or_default();
            let is_stale = !desc.file_hash.is_empty() && desc.file_hash != current_hash;

            let stale_marker = if is_stale { " [!]" } else { "" };
            output.push_str(&format!("{}{}{}{} - {}\n", prefix, connector, name_str, stale_marker, desc.description));
        } else {
            output.push_str(&format!("{}{}{}\n", prefix, connector, name_str));
        }
    }
}
