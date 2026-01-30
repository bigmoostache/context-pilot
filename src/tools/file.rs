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

    // Add to context
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
    });

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Opened '{}' as {} ({} tokens)", path, context_id, token_count),
        is_error: false,
    }
}

/// Get file contents for all open files in context
pub fn get_context_files(state: &State) -> Vec<(String, String)> {
    state
        .context
        .iter()
        .filter(|c| c.context_type == ContextType::File)
        .filter_map(|c| {
            let path = c.file_path.as_ref()?;
            let content = fs::read_to_string(path).ok()?;
            Some((path.clone(), content))
        })
        .collect()
}

/// Check all open files for changes and update hashes/token counts
/// Returns true if any file changed
pub fn refresh_file_hashes(state: &mut State) -> bool {
    let mut changed = false;

    for ctx in &mut state.context {
        if ctx.context_type != ContextType::File {
            continue;
        }

        let Some(path) = &ctx.file_path else { continue };
        let Ok(content) = fs::read_to_string(path) else { continue };

        let new_hash = hash_content(&content);

        if ctx.file_hash.as_ref() != Some(&new_hash) {
            // File changed - update hash and token count
            let new_token_count = estimate_tokens(&content);
            ctx.file_hash = Some(new_hash);
            ctx.token_count = new_token_count;
            changed = true;
        }
    }

    changed
}
