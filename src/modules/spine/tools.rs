use crate::tools::{ToolResult, ToolUse};
use crate::state::{ContextType, State};

/// Execute the notification_mark_processed tool
pub fn execute_mark_processed(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(i) => i,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required 'id' parameter".to_string(),
                is_error: true,
            };
        }
    };

    let notification = state.notifications.iter_mut().find(|n| n.id == id);

    match notification {
        Some(n) => {
            if n.processed {
                ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Notification {} is already processed", id),
                    is_error: false,
                }
            } else {
                n.processed = true;
                // Mark spine panel as needing refresh
                state.touch_panel(ContextType::Spine);
                ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Marked notification {} as processed", id),
                    is_error: false,
                }
            }
        }
        None => {
            ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Notification '{}' not found", id),
                is_error: true,
            }
        }
    }
}
