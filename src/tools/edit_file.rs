use std::fs;
use std::path::Path;

use super::{ToolResult, ToolUse};
use crate::state::{estimate_tokens, ContextElement, ContextType, State};

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

    let old_string = match tool.input.get("old_string").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'old_string' parameter".to_string(),
                is_error: true,
            }
        }
    };

    let new_string = match tool.input.get("new_string").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'new_string' parameter".to_string(),
                is_error: true,
            }
        }
    };

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
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Failed to read file '{}': {}", path, e),
                is_error: true,
            }
        }
    };

    // Count occurrences of old_string
    let match_count = content.matches(old_string).count();

    if match_count == 0 {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Error: '{}' not found in file. No matches for the replacement text.", path),
            is_error: true,
        };
    }

    if match_count > 1 {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Error: Found {} matches for replacement text. Please provide more context to make a unique match.", match_count),
            is_error: true,
        };
    }

    // Perform the replacement
    let new_content = content.replacen(old_string, new_string, 1);

    // Write back to file
    if let Err(e) = fs::write(path, &new_content) {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Failed to write file '{}': {}", path, e),
            is_error: true,
        };
    }

    // Update the context element's token count and hash
    if let Some(ctx) = state.context.iter_mut().find(|c| {
        c.context_type == ContextType::File && c.file_path.as_deref() == Some(path)
    }) {
        ctx.token_count = estimate_tokens(&new_content);
        // Hash will be updated on next refresh
    }

    let lines_changed = old_string.lines().count().max(new_string.lines().count());
    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Edited '{}' (~{} lines changed)", path, lines_changed),
        is_error: false,
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

    // Generate context ID and add to context
    let context_id = format!("P{}", state.next_context_id);
    state.next_context_id += 1;

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
