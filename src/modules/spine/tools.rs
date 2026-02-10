use crate::tools::{ToolResult, ToolUse};
use crate::state::State;

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

    // Check if notification exists and its current state
    let already_processed = state.notifications.iter().find(|n| n.id == id).map(|n| n.processed);

    match already_processed {
        Some(true) => {
            ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Notification {} is already processed", id),
                is_error: false,
            }
        }
        Some(false) => {
            state.mark_notification_processed(id);
            ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Marked notification {} as processed", id),
                is_error: false,
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
