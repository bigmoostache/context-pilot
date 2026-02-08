use std::fs;
use std::path::Path;

use crate::tools::{ToolResult, ToolUse};
use crate::state::{estimate_tokens, ContextElement, ContextType, State};

/// Result of creating a single item
enum CreateResult {
    Success { opened_folders: Vec<String> },
    AlreadyExists,
    Error(String),
}

pub fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let items = match tool.input.get("items").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'items' parameter (expected array)".to_string(),
                is_error: true,
            }
        }
    };

    if items.is_empty() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "No items provided".to_string(),
            is_error: true,
        };
    }

    let mut successes: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    for (i, item) in items.iter().enumerate() {
        let item_type = match item.get("type").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => {
                failures.push(format!("Item {}: missing 'type'", i + 1));
                continue;
            }
        };

        let path = match item.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                failures.push(format!("Item {}: missing 'path'", i + 1));
                continue;
            }
        };

        match item_type {
            "file" => {
                let content = item.get("content").and_then(|v| v.as_str()).unwrap_or("");
                match create_file(path, content, state) {
                    CreateResult::Success { opened_folders } => {
                        // Open parent folders in tree
                        for folder in opened_folders {
                            if !state.tree_open_folders.contains(&folder) {
                                state.tree_open_folders.push(folder);
                            }
                        }
                        successes.push(format!("Item {}: created file '{}'", i + 1, path));
                    }
                    CreateResult::AlreadyExists => {
                        failures.push(format!("Item {}: '{}' already exists", i + 1, path));
                    }
                    CreateResult::Error(e) => {
                        failures.push(format!("Item {}: {}", i + 1, e));
                    }
                }
            }
            "folder" => {
                match create_folder(path) {
                    CreateResult::Success { opened_folders } => {
                        // Open parent folders in tree (and the folder itself)
                        for folder in opened_folders {
                            if !state.tree_open_folders.contains(&folder) {
                                state.tree_open_folders.push(folder);
                            }
                        }
                        successes.push(format!("Item {}: created folder '{}'", i + 1, path));
                    }
                    CreateResult::AlreadyExists => {
                        failures.push(format!("Item {}: '{}' already exists", i + 1, path));
                    }
                    CreateResult::Error(e) => {
                        failures.push(format!("Item {}: {}", i + 1, e));
                    }
                }
            }
            _ => {
                failures.push(format!("Item {}: invalid type '{}' (use 'file' or 'folder')", i + 1, item_type));
            }
        }
    }

    // Invalidate tree cache so it refreshes
    invalidate_tree_cache(state);

    // Build result message
    let total_items = items.len();
    let success_count = successes.len();
    let failure_count = failures.len();

    if failure_count == 0 {
        // All succeeded
        ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Created {}/{} items: {}", success_count, total_items, successes.join("; ")),
            is_error: false,
        }
    } else if success_count == 0 {
        // All failed
        ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Failed to create items: {}", failures.join("; ")),
            is_error: true,
        }
    } else {
        // Partial success
        ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Partial success: {}/{} created. Successes: {}. Failures: {}",
                success_count, total_items, successes.join("; "), failures.join("; ")),
            is_error: false,
        }
    }
}

fn create_file(path: &str, content: &str, state: &mut State) -> CreateResult {
    let file_path = Path::new(path);

    // Check if file already exists
    if file_path.exists() {
        return CreateResult::AlreadyExists;
    }

    // Collect parent folders to open in tree
    let opened_folders = collect_parent_folders(path);

    // Create parent directories if needed
    if let Some(parent) = file_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            if let Err(e) = fs::create_dir_all(parent) {
                return CreateResult::Error(format!("Failed to create directory '{}': {}", parent.display(), e));
            }
        }
    }

    // Write the file
    if let Err(e) = fs::write(path, content) {
        return CreateResult::Error(format!("Failed to create file '{}': {}", path, e));
    }

    // Add to context with UID
    let context_id = state.next_available_context_id();
    let uid = format!("UID_{}_P", state.global_next_uid);
    state.global_next_uid += 1;

    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    let token_count = estimate_tokens(content);

    state.context.push(ContextElement {
        id: context_id,
        uid: Some(uid),
        context_type: ContextType::File,
        name: file_name,
        token_count,
        file_path: Some(path.to_string()),
        file_hash: None,
        glob_pattern: None,
        glob_path: None,
        grep_pattern: None,
        grep_path: None,
        grep_file_pattern: None,
        tmux_pane_id: None,
        tmux_lines: None,
        tmux_last_keys: None,
        tmux_description: None,
        result_command: None,
        result_command_hash: None,
        cached_content: Some(content.to_string()),
        cache_deprecated: true,
        last_refresh_ms: crate::core::panels::now_ms(),
        tmux_last_lines_hash: None,
        current_page: 0,
        total_pages: 1,
        full_token_count: 0,
    });

    CreateResult::Success { opened_folders }
}

fn create_folder(path: &str) -> CreateResult {
    let folder_path = Path::new(path);

    // Check if folder already exists
    if folder_path.exists() {
        return CreateResult::AlreadyExists;
    }

    // Collect parent folders to open, plus the folder itself
    let mut opened_folders = collect_parent_folders(path);
    // Also open the created folder itself
    opened_folders.push(path.to_string());

    // Create the folder (and parents)
    if let Err(e) = fs::create_dir_all(path) {
        return CreateResult::Error(format!("Failed to create folder '{}': {}", path, e));
    }

    CreateResult::Success { opened_folders }
}

/// Collect all parent folder paths for opening in tree
fn collect_parent_folders(path: &str) -> Vec<String> {
    let mut folders = Vec::new();
    let file_path = Path::new(path);
    let mut current = file_path.parent();

    while let Some(parent) = current {
        let parent_str = parent.to_string_lossy().to_string();
        if parent_str.is_empty() || parent_str == "." {
            break;
        }
        folders.push(parent_str);
        current = parent.parent();
    }

    folders
}

/// Invalidate tree cache so it refreshes to show new files/folders
fn invalidate_tree_cache(state: &mut State) {
    for ctx in &mut state.context {
        if ctx.context_type == ContextType::Tree {
            ctx.cache_deprecated = true;
        }
    }
}
