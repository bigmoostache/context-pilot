use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

use super::{ToolResult, ToolUse};
use crate::state::{estimate_tokens, ContextElement, ContextType, State};

fn hash_content(content: &str) -> String {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub fn execute_open(tool: &ToolUse, state: &mut State) -> ToolResult {
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

    // Check if file is already open
    if state.context.iter().any(|c| c.file_path.as_deref() == Some(path)) {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("File '{}' is already open in context", path),
            is_error: false,
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

    let hash = hash_content(&content);
    let token_count = estimate_tokens(&content);
    let file_name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    // Generate context ID
    let context_id = format!("P{}", state.next_context_id);
    state.next_context_id += 1;

    // Add to context with cached content
    state.context.push(ContextElement {
        id: context_id.clone(),
        context_type: ContextType::File,
        name: file_name,
        token_count,
        file_path: Some(path.to_string()),
        file_hash: Some(hash),
        glob_pattern: None,
        glob_path: None,
        grep_pattern: None,
        grep_path: None,
        grep_file_pattern: None,
        tmux_pane_id: None,
        tmux_lines: None,
        tmux_last_keys: None,
        tmux_description: None,
        cached_content: Some(content),
        cache_deprecated: false,
        last_refresh_ms: 0,
        tmux_last_lines_hash: None,
    });

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Opened '{}' as {} ({} tokens)", path, context_id, token_count),
        is_error: false,
    }
}
