use std::fs;
use std::path::Path;

use super::{ToolResult, ToolUse};
use crate::state::{estimate_tokens, ContextElement, ContextType, State};

/// Result of applying a single edit
enum EditResult {
    Success { lines_changed: usize },
    NoMatch,
    MultipleMatches(usize),
}

pub fn execute_edit(tool: &ToolUse, state: &mut State) -> ToolResult {
    let path = match tool.input.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'path' parameter".to_string(),
                is_error: true,
            }
        }
    };

    let edits = match tool.input.get("edits").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'edits' parameter (expected array)".to_string(),
                is_error: true,
            }
        }
    };

    if edits.is_empty() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "No edits provided".to_string(),
            is_error: true,
        };
    }

    // Check if file is open in context
    let is_open = state.context.iter().any(|c| {
        c.context_type == ContextType::File && c.file_path.as_deref() == Some(path)
    });

    if !is_open {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("File '{}' is not open in context. Use open_file first.", path),
            is_error: true,
        };
    }

    // Read the file
    let mut content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Failed to read file '{}': {}", path, e),
                is_error: true,
            }
        }
    };

    // Apply edits sequentially
    let mut successes: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    let mut total_lines_changed = 0;

    for (i, edit) in edits.iter().enumerate() {
        let old_string = match edit.get("old_string").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                failures.push(format!("Edit {}: missing 'old_string'", i + 1));
                continue;
            }
        };

        let new_string = match edit.get("new_string").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                failures.push(format!("Edit {}: missing 'new_string'", i + 1));
                continue;
            }
        };

        // Apply this edit to the current content
        match apply_single_edit(&content, old_string, new_string) {
            EditResult::Success { lines_changed } => {
                content = content.replacen(old_string, new_string, 1);
                total_lines_changed += lines_changed;
                successes.push(format!("Edit {}: ~{} lines", i + 1, lines_changed));
            }
            EditResult::NoMatch => {
                failures.push(format!("Edit {}: no match found", i + 1));
            }
            EditResult::MultipleMatches(count) => {
                failures.push(format!("Edit {}: {} matches (need unique)", i + 1, count));
            }
        }
    }

    // Only write if at least one edit succeeded
    if !successes.is_empty() {
        if let Err(e) = fs::write(path, &content) {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Failed to write file '{}': {}", path, e),
                is_error: true,
            };
        }

        // Update the context element's token count
        if let Some(ctx) = state.context.iter_mut().find(|c| {
            c.context_type == ContextType::File && c.file_path.as_deref() == Some(path)
        }) {
            ctx.token_count = estimate_tokens(&content);
        }
    }

    // Build result message
    let total_edits = edits.len();
    let success_count = successes.len();
    let failure_count = failures.len();

    if failure_count == 0 {
        // All succeeded
        ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Edited '{}': {}/{} edits applied (~{} lines changed)",
                path, success_count, total_edits, total_lines_changed),
            is_error: false,
        }
    } else if success_count == 0 {
        // All failed
        ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Failed to edit '{}': {}", path, failures.join("; ")),
            is_error: true,
        }
    } else {
        // Partial success
        ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Partial edit '{}': {}/{} applied. Failed: {}",
                path, success_count, total_edits, failures.join("; ")),
            is_error: false, // Not a full error since some succeeded
        }
    }
}

fn apply_single_edit(content: &str, old_string: &str, new_string: &str) -> EditResult {
    let match_count = content.matches(old_string).count();

    if match_count == 0 {
        EditResult::NoMatch
    } else if match_count > 1 {
        EditResult::MultipleMatches(match_count)
    } else {
        let lines_changed = old_string.lines().count().max(new_string.lines().count());
        EditResult::Success { lines_changed }
    }
}

pub fn execute_create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let path = match tool.input.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'path' parameter".to_string(),
                is_error: true,
            }
        }
    };

    let contents = match tool.input.get("contents").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'contents' parameter".to_string(),
                is_error: true,
            }
        }
    };

    // Check if file already exists
    if Path::new(path).exists() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("File '{}' already exists. Use edit_file to modify it.", path),
            is_error: true,
        };
    }

    // Create parent directories if needed
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = fs::create_dir_all(parent) {
                return ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Failed to create directory '{}': {}", parent.display(), e),
                    is_error: true,
                };
            }
        }
    }

    // Write the file
    if let Err(e) = fs::write(path, contents) {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Failed to create file '{}': {}", path, e),
            is_error: true,
        };
    }

    // Generate context ID (fills gaps)
    let context_id = state.next_available_context_id();

    let file_name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    let token_count = estimate_tokens(contents);

    state.context.push(ContextElement {
        id: context_id.clone(),
        context_type: ContextType::File,
        name: file_name,
        token_count,
        file_path: Some(path.to_string()),
        file_hash: None, // Will be computed by background cache
        glob_pattern: None,
        glob_path: None,
        grep_pattern: None,
        grep_path: None,
        grep_file_pattern: None,
        tmux_pane_id: None,
        tmux_lines: None,
        tmux_last_keys: None,
        tmux_description: None,
        cached_content: Some(contents.to_string()),
        cache_deprecated: true, // Mark as deprecated so background computes hash
        last_refresh_ms: 0,
        tmux_last_lines_hash: None,
    });

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Created '{}' as {} ({} tokens)", path, context_id, token_count),
        is_error: false,
    }
}
