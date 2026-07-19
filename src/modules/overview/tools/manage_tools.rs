use crate::infra::tools::{ToolResult, ToolUse};
use crate::state::State;

/// The ID of this tool - it cannot be disabled
pub(crate) const MANAGE_TOOLS_ID: &str = "manage_tools";

/// Apply one enable/disable change. Returns `Ok(msg)` on success (including
/// no-op "already enabled") or `Err(msg)` on validation failure. `idx` is the
/// zero-based change index (for human-readable error prefixes).
fn apply_one_change(state: &mut State, change: &serde_json::Value, idx: usize) -> Result<String, String> {
    let n = idx.saturating_add(1);
    let Some(tool_name) = change.get("tool").and_then(serde_json::Value::as_str) else {
        return Err(format!("Change {n}: missing 'tool'"));
    };
    let Some(action) = change.get("action").and_then(serde_json::Value::as_str) else {
        return Err(format!("Change {n}: missing 'action'"));
    };
    if tool_name == MANAGE_TOOLS_ID && action == "disable" {
        return Err(format!("Change {n}: cannot disable '{MANAGE_TOOLS_ID}'"));
    }
    if tool_name == "panel_goto_page" {
        return Err(format!("Change {n}: '{tool_name}' is automatically managed (enabled when panels are paginated)"));
    }
    let Some(t) = state.tools.iter_mut().find(|t| t.id == tool_name) else {
        return Err(format!("Change {n}: tool '{tool_name}' not found"));
    };
    match action {
        "enable" if t.enabled => Ok(format!("'{tool_name}' already enabled")),
        "enable" => {
            t.enabled = true;
            Ok(format!("enabled '{tool_name}'"))
        }
        "disable" if t.enabled => {
            t.enabled = false;
            Ok(format!("disabled '{tool_name}'"))
        }
        "disable" => Ok(format!("'{tool_name}' already disabled")),
        _ => Err(format!("Change {n}: invalid action '{action}' (use 'enable' or 'disable')")),
    }
}

/// Execute the `tool_manage` tool to enable or disable tools.
pub(crate) fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(changes) = tool.input.get("changes").and_then(serde_json::Value::as_array) else {
        return ToolResult::new(tool.id.clone(), "Missing 'changes' parameter (expected array)".to_owned(), true);
    };

    if changes.is_empty() {
        return ToolResult::new(tool.id.clone(), "No changes provided".to_owned(), true);
    }

    let mut successes: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    for (i, change) in changes.iter().enumerate() {
        match apply_one_change(state, change, i) {
            Ok(msg) => successes.push(msg),
            Err(msg) => failures.push(msg),
        }
    }

    // Build result message
    let total_changes = changes.len();
    let success_count = successes.len();
    let failure_count = failures.len();

    if failure_count == 0 {
        ToolResult::new(
            tool.id.clone(),
            format!("Tool changes: {}/{} applied ({})", success_count, total_changes, successes.join("; ")),
            false,
        )
    } else if success_count == 0 {
        ToolResult::new(tool.id.clone(), format!("Failed to apply changes: {}", failures.join("; ")), true)
    } else {
        ToolResult::new(
            tool.id.clone(),
            format!(
                "Partial success: {}/{} applied. Successes: {}. Failures: {}",
                success_count,
                total_changes,
                successes.join("; "),
                failures.join("; ")
            ),
            false,
        )
    }
}
