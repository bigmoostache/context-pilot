use super::{ToolResult, ToolUse};
use crate::persistence::save_message;
use crate::state::{MessageStatus, State};

pub fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let changes = match tool.input.get("changes").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'changes' array".to_string(),
                is_error: true,
            };
        }
    };

    if changes.is_empty() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "Empty 'changes' array".to_string(),
            is_error: true,
        };
    }

    let mut results: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for change in changes {
        let message_id = match change.get("message_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => {
                errors.push("Missing 'message_id'".to_string());
                continue;
            }
        };

        let status_str = match change.get("status").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                errors.push(format!("{}: missing 'status'", message_id));
                continue;
            }
        };

        let new_status = match status_str {
            "full" => MessageStatus::Full,
            "summarized" => MessageStatus::Summarized,
            "deleted" => MessageStatus::Deleted,
            _ => {
                errors.push(format!("{}: invalid status '{}'", message_id, status_str));
                continue;
            }
        };

        let msg = match state.messages.iter_mut().find(|m| m.id == message_id) {
            Some(m) => m,
            None => {
                errors.push(format!("{}: not found", message_id));
                continue;
            }
        };

        if new_status == MessageStatus::Summarized {
            if let Some(tl_dr) = change.get("tl_dr").and_then(|v| v.as_str()) {
                msg.tl_dr = Some(tl_dr.to_string());
            } else if msg.tl_dr.is_none() {
                errors.push(format!("{}: no tl_dr provided", message_id));
                continue;
            }
        }

        let old_status = msg.status;
        msg.status = new_status;
        save_message(msg);

        results.push(format!("{}: {:?} -> {:?}", message_id, old_status, new_status));
    }

    let mut output = String::new();

    if !results.is_empty() {
        output.push_str(&format!("Updated {}:\n{}", results.len(), results.join("\n")));
    }

    if !errors.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str(&format!("Errors ({}):\n{}", errors.len(), errors.join("\n")));
    }

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: output,
        is_error: !errors.is_empty() && results.is_empty(),
    }
}
