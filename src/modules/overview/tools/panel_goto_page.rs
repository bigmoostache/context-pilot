use crate::app::panels::paginate_content;
use crate::infra::tools::{ToolResult, ToolUse};
use crate::state::{State, estimate_tokens};
use cp_base::cast::Safe as _;
/// Execute the `panel_goto_page` tool to navigate paginated panels.
pub(crate) fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(panel_id) = tool.input.get("panel_id").and_then(serde_json::Value::as_str) else {
        return ToolResult::new(tool.id.clone(), "Missing 'panel_id' parameter".to_string(), true);
    };

    let Some(page) = tool.input.get("page").and_then(serde_json::Value::as_i64) else {
        return ToolResult::new(tool.id.clone(), "Missing 'page' parameter (expected integer)".to_string(), true);
    };

    // Find the context element by panel ID
    let Some(ctx) = state.context.iter_mut().find(|c| c.id == panel_id) else {
        return ToolResult::new(tool.id.clone(), format!("Panel '{panel_id}' not found"), true);
    };

    if ctx.total_pages <= 1 {
        return ToolResult::new(
            tool.id.clone(),
            format!("Panel '{panel_id}' has only 1 page — no pagination needed"),
            true,
        );
    }

    if page < 1 || page.to_usize() > ctx.total_pages {
        return ToolResult::new(
            tool.id.clone(),
            format!("Page {} out of range for panel '{}' (valid: 1-{})", page, panel_id, ctx.total_pages),
            true,
        );
    }

    ctx.current_page = page.saturating_sub(1).to_usize();

    // Recompute token_count for the new page
    if let Some(content) = &ctx.cached_content {
        let page_content = paginate_content(content, ctx.current_page, ctx.total_pages);
        ctx.token_count = estimate_tokens(&page_content);
    }

    ToolResult::new(
        tool.id.clone(),
        format!("Panel '{}' now showing page {}/{}", panel_id, page, ctx.total_pages),
        false,
    )
}
