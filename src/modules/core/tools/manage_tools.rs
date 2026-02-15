use crate::state::State;
use crate::tools::{ToolResult, ToolUse};

/// The ID of this tool - it cannot be disabled
pub const MANAGE_TOOLS_ID: &str = "manage_tools";

pub fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let changes = match tool.input.get("changes").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'changes' parameter (expected array)".to_string(),
                is_error: true,
            };
        }
    };

    if changes.is_empty() {
        return ToolResult { tool_use_id: tool.id.clone(), content: "No changes provided".to_string(), is_error: true };
    }

    let mut successes: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    for (i, change) in changes.iter().enumerate() {
        let tool_name = match change.get("tool").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => {
                failures.push(format!("Change {}: missing 'tool'", i + 1));
                continue;
            }
        };

        let action = match change.get("action").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => {
                failures.push(format!("Change {}: missing 'action'", i + 1));
                continue;
            }
        };

        // Cannot disable the manage_tools tool itself
        if tool_name == MANAGE_TOOLS_ID && action == "disable" {
            failures.push(format!("Change {}: cannot disable '{}'", i + 1, MANAGE_TOOLS_ID));
            continue;
        }

        // panel_goto_page is system-managed — cannot be manually toggled
        if tool_name == "panel_goto_page" {
            failures.push(format!(
                "Change {}: '{}' is automatically managed (enabled when panels are paginated)",
                i + 1,
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
                        successes.push(format!("'{}' already enabled", tool_name));
                    } else {
                        t.enabled = true;
                        successes.push(format!("enabled '{}'", tool_name));
                    }
                }
                "disable" => {
                    if !t.enabled {
                        successes.push(format!("'{}' already disabled", tool_name));
                    } else {
                        t.enabled = false;
                        successes.push(format!("disabled '{}'", tool_name));
                    }
                }
                _ => {
                    failures.push(format!("Change {}: invalid action '{}' (use 'enable' or 'disable')", i + 1, action));
                }
            },
            None => {
                failures.push(format!("Change {}: tool '{}' not found", i + 1, tool_name));
            }
        }
    }

    // Build result message
    let total_changes = changes.len();
    let success_count = successes.len();
    let failure_count = failures.len();

    if failure_count == 0 {
        ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Tool changes: {}/{} applied ({})", success_count, total_changes, successes.join("; ")),
            is_error: false,
        }
    } else if success_count == 0 {
        ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Failed to apply changes: {}", failures.join("; ")),
            is_error: true,
        }
    } else {
        ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!(
                "Partial success: {}/{} applied. Successes: {}. Failures: {}",
                success_count,
                total_changes,
                successes.join("; "),
                failures.join("; ")
            ),
            is_error: false,
        }
    }
}
