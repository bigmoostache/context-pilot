use std::path::Path;

use crate::tools::{ToolResult, ToolUse};
use crate::state::{ContextElement, ContextType, State};

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

    // Check if file exists (quick metadata check, not a full read)
    let path_obj = Path::new(path);
    if !path_obj.exists() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("File '{}' not found", path),
            is_error: true,
        };
    }

    if !path_obj.is_file() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("'{}' is not a file", path),
            is_error: true,
        };
    }

    let file_name = path_obj
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    // Generate context ID (fills gaps) and UID
    let context_id = state.next_available_context_id();
    let uid = format!("UID_{}_P", state.global_next_uid);
    state.global_next_uid += 1;

    // Create context element WITHOUT reading file content
    // Background cache system will populate it
    state.context.push(ContextElement {
        id: context_id.clone(),
        uid: Some(uid),
        context_type: ContextType::File,
        name: file_name,
        token_count: 0, // Will be updated by cache
        file_path: Some(path.to_string()),
        file_hash: None, // Will be computed by cache
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
        cached_content: None, // Background will populate
        cache_deprecated: true, // Trigger background refresh
        last_refresh_ms: crate::core::panels::now_ms(),
        tmux_last_lines_hash: None,
        current_page: 0,
        total_pages: 1,
        full_token_count: 0,
    });

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Opened '{}' as {}", path, context_id),
        is_error: false,
    }
}
