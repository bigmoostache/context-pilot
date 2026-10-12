use std::fs;

use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};
use std::fmt::Write as _;

/// Create `path`'s missing parent directories; error message on failure.
fn ensure_parent_dir(path: &std::path::Path) -> Result<(), String> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() && !parent.exists() => {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory '{}': {}", parent.display(), e))
        }
        _ => Ok(()),
    }
}

/// Execute the Write tool: create or overwrite a file and update context.
pub(crate) fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("file_write");
    let Some(path_str) = tool.input.get("file_path").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing required parameter: file_path".to_owned(), true);
    };

    let Some(contents) = tool.input.get("contents").or_else(|| tool.input.get("content")).and_then(|v| v.as_str())
    else {
        return ToolResult::new(tool.id.clone(), "Missing required parameter: contents".to_owned(), true);
    };

    // try_: create_dir_all below would otherwise silently recreate a deleted worktree.
    let resolved = match state.try_resolve_path(path_str) {
        Ok(p) => p,
        Err(e) => return ToolResult::new(tool.id.clone(), format!("Error: {e}"), true),
    };
    let path = resolved.as_path();
    let is_new = !path.exists();

    if let Err(msg) = ensure_parent_dir(path) {
        return ToolResult::new(tool.id.clone(), msg, true);
    }

    // Write the file
    if let Err(e) = fs::write(path, contents) {
        return ToolResult::new(tool.id.clone(), format!("Failed to write file '{path_str}': {e}"), true);
    }

    let token_count = estimate_tokens(contents);
    let line_count = contents.lines().count();
    // Absolute key, so panels match Open/Edit and survive a workdir change.
    let canonical =
        path.canonicalize().map_or_else(|_| path.to_string_lossy().to_string(), |p| p.to_string_lossy().to_string());

    // Check if file is already open in context
    let already_open = state.thread_mut().context.iter_mut().find(|c| {
        c.context_type.as_str() == Kind::FILE
            && matches!(c.get_meta_str("file_path"), Some(p) if p == canonical || p == path_str)
    });

    if let Some(ctx) = already_open {
        // Update existing context element
        ctx.token_count = token_count;
        ctx.cache_deprecated = true;
    } else {
        // Add new context element
        let context_id = state.next_available_context_id();
        let uid = format!("UID_{}_P", state.global_next_uid);
        state.global_next_uid = state.global_next_uid.saturating_add(1);

        let file_name = path.file_name().map_or_else(|| path_str.to_owned(), |n| n.to_string_lossy().to_string());

        let mut elem =
            cp_base::state::context::make_default_entry(&context_id, Kind::new(Kind::FILE), &file_name, true);
        elem.uid = Some(uid);
        elem.token_count = token_count;
        elem.cached_content = Some(contents.to_owned());
        elem.set_meta("file_path", &canonical);
        state.thread_mut().context.push(elem);

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
