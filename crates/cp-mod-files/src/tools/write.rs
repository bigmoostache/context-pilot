use std::fs;
use std::path::Path;

use cp_base::state::context::{Entry, Kind, estimate_tokens};
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};
use std::fmt::Write as _;

/// Execute the Write tool: create or overwrite a file and update context.
pub(crate) fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(path_str) = tool.input.get("file_path").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing required parameter: file_path".to_string(), true);
    };

    let Some(contents) = tool.input.get("contents").or_else(|| tool.input.get("content")).and_then(|v| v.as_str())
    else {
        return ToolResult::new(tool.id.clone(), "Missing required parameter: contents".to_string(), true);
    };

    let path = Path::new(path_str);
    let is_new = !path.exists();

    // Create parent directories if needed
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
        && let Err(e) = fs::create_dir_all(parent)
    {
        return ToolResult::new(
            tool.id.clone(),
            format!("Failed to create directory '{}': {}", parent.display(), e),
            true,
        );
    }

    // Write the file
    if let Err(e) = fs::write(path, contents) {
        return ToolResult::new(tool.id.clone(), format!("Failed to write file '{path_str}': {e}"), true);
    }

    let token_count = estimate_tokens(contents);
    let line_count = contents.lines().count();

    // Check if file is already open in context
    let already_open = state
        .context
        .iter_mut()
        .find(|c| c.context_type.as_str() == Kind::FILE && c.get_meta_str("file_path") == Some(path_str));

    if let Some(ctx) = already_open {
        // Update existing context element
        ctx.token_count = token_count;
        ctx.cache_deprecated = true;
    } else {
        // Add new context element
        let context_id = state.next_available_context_id();
        let uid = format!("UID_{}_P", state.global_next_uid);
        state.global_next_uid = state.global_next_uid.saturating_add(1);

        let file_name = path.file_name().map_or_else(|| path_str.to_string(), |n| n.to_string_lossy().to_string());

        let mut elem = Entry {
            id: context_id,
            uid: Some(uid),
            context_type: Kind::new(Kind::FILE),
            name: file_name,
            token_count,
            metadata: std::collections::HashMap::new(),
            cached_content: Some(contents.to_string()),
            history_messages: None,
            cache_deprecated: true,
            cache_in_flight: false,
            last_refresh_ms: cp_base::panels::now_ms(),
            content_hash: None,
            source_hash: None,
            current_page: 0,
            total_pages: 1,
            full_token_count: 0,
            panel_cache_hit: false,
            panel_total_cost: 0.0,
            freeze_count: 0,
            total_freezes: 0,
            total_cache_misses: 0,
            last_emitted_content: None,
            last_emitted_hash: None,
            last_emitted_context: None,
        };
        elem.set_meta("file_path", &path_str.to_string());
        state.context.push(elem);

        // Invalidate tree cache
        cp_base::panels::mark_panels_dirty(state, Kind::TREE);
    }

    let action = if is_new { "Created" } else { "Wrote" };
    let mut result_msg = format!("{action} '{path_str}' ({line_count} lines, {token_count} tokens)\n");

    // Add diff-style preview of written content (truncated for large files)
    result_msg.push_str("```diff\n");
    for (i, line) in contents.lines().enumerate() {
        if i >= 20 {
            let _r = writeln!(result_msg, "+ ... ({} more lines)", line_count.saturating_sub(20));
            break;
        }
        let _r = writeln!(result_msg, "+ {line}");
    }
    result_msg.push_str("```");

    ToolResult::new(tool.id.clone(), result_msg, false)
}
