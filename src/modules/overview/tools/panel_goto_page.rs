use crate::app::panels::paginate_content;
use crate::infra::tools::{ToolResult, ToolUse};
use crate::state::{State, estimate_tokens};
use cp_base::cast::Safe as _;
/// Execute the `panel_goto_page` tool to navigate paginated panels.
pub(crate) fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(panel_id) = tool.input.get("panel_id").and_then(serde_json::Value::as_str) else {
        return ToolResult::new(tool.id.clone(), "Missing 'panel_id' parameter".to_owned(), true);
    };

    let Some(page) = tool.input.get("page").and_then(serde_json::Value::as_i64) else {
        return ToolResult::new(tool.id.clone(), "Missing 'page' parameter (expected integer)".to_owned(), true);
    };

    // Compulsory: the LLM must record what it saw on the page it is LEAVING.
    // This note is saved to the panel's scratchpad so the information survives
    // after the current page's raw content is discarded from context.
    let description =
        tool.input.get("current_page_description").and_then(serde_json::Value::as_str).unwrap_or("").trim();
    if description.is_empty() {
        return ToolResult::new(
            tool.id.clone(),
            "Missing 'current_page_description' \u{2014} you MUST summarize what you see on the CURRENT page \
(the one you are leaving) before navigating. Its raw content will be discarded; this note is all you keep."
                .to_owned(),
            true,
        );
    }

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

    // Save the note for the page we are LEAVING, then navigate.
    drop(ctx.page_descriptions.insert(ctx.current_page, description.to_owned()));

    ctx.current_page = page.saturating_sub(1).to_usize();

    // Recompute token_count for the new page
    if let Some(content) = &ctx.cached_content {
        let page_content = paginate_content(content, ctx.current_page, ctx.total_pages, &ctx.page_descriptions);
        ctx.token_count = estimate_tokens(&page_content);
    }

    ToolResult::new(
        tool.id.clone(),
        format!("Panel '{}' now showing page {}/{}", panel_id, page, ctx.total_pages),
        false,
    )
}
