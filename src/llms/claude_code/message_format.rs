//! Message preparation for Claude Code's API.
//!
//! Handles user/assistant alternation, system-reminder injection, and
//! content-block normalization required by the Claude Code server.

use serde_json::Value;

use super::SYSTEM_REMINDER;

/// Sentinel value returned by `.get()` when a key is missing.
const NULL: Value = Value::Null;

/// Inject the system-reminder text block into the first non-tool-result user message.
/// Claude Code's server validates that messages contain this marker.
/// Must skip `tool_result` user messages (from panel injection) since mixing text blocks
/// into `tool_result` messages breaks the API's `tool_use/tool_result` pairing.
pub(super) fn inject_system_reminder(messages: &mut Vec<Value>) {
    let reminder = serde_json::json!({"type": "text", "text": SYSTEM_REMINDER});

    for msg in messages.iter_mut() {
        if msg.get("role").unwrap_or(&NULL) != "user" {
            continue;
        }

        // Skip tool_result messages (from panel injection / tool loop)
        if let Some(arr) = msg.get("content").unwrap_or(&NULL).as_array()
            && arr.iter().any(|block| block.get("type").unwrap_or(&NULL) == "tool_result")
        {
            continue;
        }

        // Convert string content to array format and prepend reminder
        let content = msg.get("content").unwrap_or(&NULL);
        if content.is_string() {
            let text = content.as_str().unwrap_or("").to_string();
            msg["content"] = serde_json::json!([
                reminder,
                {"type": "text", "text": text}
            ]);
        } else if content.is_array()
            && let Some(arr) = msg.get_mut("content").and_then(Value::as_array_mut)
        {
            arr.insert(0, reminder);
        }
        return; // Only inject into first eligible user message
    }

    // No eligible user message found (all are tool_results, e.g. during tool loop).
    // Prepend a standalone user message with just the reminder at position 0.
    messages.insert(
        0,
        serde_json::json!({
            "role": "user",
            "content": [reminder]
        }),
    );
    // Must follow with a minimal assistant ack to maintain user/assistant alternation.
    messages.insert(
        1,
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "ok"}]
        }),
    );
}
