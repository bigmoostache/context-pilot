use crate::infra::tools::{ToolResult, ToolUse};
use crate::state::State;

/// The ID of this tool - it cannot be disabled
pub(crate) const MANAGE_TOOLS_ID: &str = "manage_tools";

/// Execute the `tool_manage` tool to enable or disable tools.
pub(crate) fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(changes) = tool.input.get("changes").and_then(serde_json::Value::as_array) else {
        return ToolResult::new(tool.id.clone(), "Missing 'changes' parameter (expected array)".to_string(), true);
    };

    if changes.is_empty() {
        return ToolResult::new(tool.id.clone(), "No changes provided".to_string(), true);
    }

    let mut successes: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    for (i, change) in changes.iter().enumerate() {
        let Some(tool_name) = change.get("tool").and_then(serde_json::Value::as_str) else {
            failures.push(format!("Change {}: missing 'tool'", i.saturating_add(1)));
            continue;
        };

        let Some(action) = change.get("action").and_then(serde_json::Value::as_str) else {
            failures.push(format!("Change {}: missing 'action'", i.saturating_add(1)));
            continue;
        };

        // Cannot disable the manage_tools tool itself
        if tool_name == MANAGE_TOOLS_ID && action == "disable" {
            failures.push(format!("Change {}: cannot disable '{}'", i.saturating_add(1), MANAGE_TOOLS_ID));
            continue;
        }

        // panel_goto_page is system-managed — cannot be manually toggled
        if tool_name == "panel_goto_page" {
            failures.push(format!(
                "Change {}: '{}' is automatically managed (enabled when panels are paginated)",
                i.saturating_add(1),
                tool_name
            ));
            continue;
        }

        // Find the tool
        let tool_entry = state.tools.iter_mut().find(|t| t.id == tool_name);

        match tool_entry {
            Some(t) => match action {
                "enable" => {
                    if t.enabled {
                        successes.push(format!("'{tool_name}' already enabled"));
                    } else {
                        t.enabled = true;
                        successes.push(format!("enabled '{tool_name}'"));
                    }
                }
                "disable" => {
                    if t.enabled {
                        t.enabled = false;
                        successes.push(format!("disabled '{tool_name}'"));
                    } else {
                        successes.push(format!("'{tool_name}' already disabled"));
                    }
                }
                _ => {
                    failures.push(format!(
                        "Change {}: invalid action '{}' (use 'enable' or 'disable')",
                        i.saturating_add(1),
                        action
                    ));
                }
            },
            None => {
                failures.push(format!("Change {}: tool '{}' not found", i.saturating_add(1), tool_name));
            }
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
