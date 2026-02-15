use crate::core::panels::paginate_content;
use crate::state::{State, estimate_tokens};
use crate::tools::{ToolResult, ToolUse};

pub fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let panel_id = match tool.input.get("panel_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'panel_id' parameter".to_string(),
                is_error: true,
            };
        }
    };

    let page = match tool.input.get("page").and_then(|v| v.as_i64()) {
        Some(p) => p,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'page' parameter (expected integer)".to_string(),
                is_error: true,
            };
        }
    };

    // Find the context element by panel ID
    let ctx = match state.context.iter_mut().find(|c| c.id == panel_id) {
        Some(c) => c,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Panel '{}' not found", panel_id),
                is_error: true,
            };
        }
    };

    if ctx.total_pages <= 1 {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Panel '{}' has only 1 page — no pagination needed", panel_id),
            is_error: true,
        };
    }

    if page < 1 || page as usize > ctx.total_pages {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Page {} out of range for panel '{}' (valid: 1-{})", page, panel_id, ctx.total_pages),
            is_error: true,
        };
    }

    ctx.current_page = (page - 1) as usize;

    // Recompute token_count for the new page
    if let Some(content) = &ctx.cached_content {
        let page_content = paginate_content(content, ctx.current_page, ctx.total_pages);
        ctx.token_count = estimate_tokens(&page_content);
    }

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Panel '{}' now showing page {}/{}", panel_id, page, ctx.total_pages),
        is_error: false,
    }
}
