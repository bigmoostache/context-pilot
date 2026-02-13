use std::fs;
use std::path::Path;

use crate::tools::{ToolResult, ToolUse};
use crate::state::{estimate_tokens, ContextElement, ContextType, State};

pub fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let path_str = match tool.input.get("file_path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required parameter: file_path".to_string(),
                is_error: true,
            }
        }
    };

    let contents = match tool.input.get("contents").or_else(|| tool.input.get("content")).and_then(|v| v.as_str()) {
        Some(c) => c,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required parameter: contents".to_string(),
                is_error: true,
            }
        }
    };

    let path = Path::new(path_str);
    let is_new = !path.exists();

    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
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
            content: format!("Failed to write file '{}': {}", path_str, e),
            is_error: true,
        };
    }

    let token_count = estimate_tokens(contents);
    let line_count = contents.lines().count();

    // Check if file is already open in context
    let already_open = state.context.iter_mut().find(|c| {
        c.context_type == ContextType::File && c.file_path.as_deref() == Some(path_str)
    });

    if let Some(ctx) = already_open {
        // Update existing context element
        ctx.token_count = token_count;
        ctx.cache_deprecated = true;
    } else {
        // Add new context element
        let context_id = state.next_available_context_id();
        let uid = format!("UID_{}_P", state.global_next_uid);
        state.global_next_uid += 1;

        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path_str.to_string());

        state.context.push(ContextElement {
            id: context_id,
            uid: Some(uid),
            context_type: ContextType::File,
            name: file_name,
            token_count,
            file_path: Some(path_str.to_string()),
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
            skill_prompt_id: None,
            cached_content: Some(contents.to_string()),
            history_messages: None,
            cache_deprecated: true,
            cache_in_flight: false,
            last_refresh_ms: crate::core::panels::now_ms(),
            last_polled_ms: 0,
            content_hash: None,
            tmux_last_lines_hash: None,
            current_page: 0,
            total_pages: 1,
            full_token_count: 0,
        });

        // Invalidate tree cache
        for ctx in &mut state.context {
            if ctx.context_type == ContextType::Tree {
                ctx.cache_deprecated = true;
            }
        }
    }

    let action = if is_new { "Created" } else { "Wrote" };
    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("{} '{}' ({} lines, {} tokens)", action, path_str, line_count, token_count),
        is_error: false,
    }
}
