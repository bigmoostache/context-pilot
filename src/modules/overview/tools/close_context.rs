use crate::infra::tools::{ToolResult, ToolUse};
use crate::modules::all_modules;
use crate::state::State;
use std::fmt::Write as _;

/// Execute the `Close_panel` tool to remove context panels.
pub(crate) fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(ids) = tool.input.get("ids").and_then(serde_json::Value::as_array) else {
        return ToolResult::new(tool.id.clone(), "Missing 'ids' array parameter".to_string(), true);
    };

    if ids.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'ids' array".to_string(), true);
    }

    let mut closed: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut not_found: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    let modules = all_modules();

    for id_value in ids {
        let Some(id) = id_value.as_str() else {
            errors.push("Invalid ID (not a string)".to_string());
            continue;
        };

        // Find the context element
        let ctx_idx = state.context.iter().position(|c| c.id == id);

        let Some(idx) = ctx_idx else {
            not_found.push(id.to_string());
            continue;
        };

        // Fixed panels are always protected
        let Some(ctx_elem) = state.context.get(idx) else {
            not_found.push(id.to_string());
            continue;
        };
        if ctx_elem.context_type.is_fixed() {
            skipped.push(format!("{id} (protected)"));
            continue;
        }

        // Take the context element out so modules can mutate state without borrow conflicts
        let ctx = state.context.remove(idx);

        // Ask modules for special close handling
        let mut close_result: Option<Result<String, String>> = None;
        for module in &modules {
            if let Some(result) = module.on_close_context(&ctx, state) {
                close_result = Some(result);
                break;
            }
        }

        match close_result {
            Some(Ok(desc)) => {
                // Context already removed
                closed.push(format!("{id} ({desc})"));
            }
            Some(Err(msg)) => {
                // Put it back — close was rejected
                state.context.insert(idx, ctx);
                skipped.push(msg);
            }
            None => {
                // Default: use context_detail for description
                let detail = modules.iter().find_map(|m| m.context_detail(&ctx)).unwrap_or_else(|| ctx.name.clone());
                // Context already removed
                closed.push(format!("{id} ({detail})"));
            }
        }
    }

    // Build response
    let mut output = String::new();

    if !closed.is_empty() {
        let _r = write!(output, "Closed {}:\n{}", closed.len(), closed.join("\n"));
    }

    if !skipped.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        let _r = write!(output, "Skipped {}:\n{}", skipped.len(), skipped.join("\n"));
    }

    if !not_found.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        let _r = write!(output, "Not found: {}", not_found.join(", "));
    }

    if !errors.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        let _r = write!(output, "Errors:\n{}", errors.join("\n"));
    }

    ToolResult::new(tool.id.clone(), output, closed.is_empty() && skipped.is_empty())
}
