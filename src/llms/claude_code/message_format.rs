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

/// Ensure strict user/assistant message alternation as required by the API.
/// - Consecutive text-only user messages are merged into one.
/// - Between a `tool_result` user message and a text user message, a placeholder
///   assistant message is inserted (can't merge these — `tool_result` + text mixing
///   breaks `inject_system_reminder` and API validation).
/// - Consecutive assistant messages are merged.
pub(super) fn ensure_message_alternation(messages: &mut Vec<Value>) {
    if messages.len() <= 1 {
        return;
    }

    let mut result: Vec<Value> = Vec::with_capacity(messages.len());

    for msg in messages.drain(..) {
        let msg_role = msg.get("role").unwrap_or(&NULL);
        let same_role = result.last().is_some_and(|last: &Value| last.get("role").unwrap_or(&NULL) == msg_role);
        if !same_role {
            let blocks = content_to_blocks(msg.get("content").unwrap_or(&NULL));
            result.push(serde_json::json!({"role": msg_role, "content": blocks}));
            continue;
        }

        let prev_has_tool_result = result.last().is_some_and(|last| {
            last.get("content")
                .unwrap_or(&NULL)
                .as_array()
                .is_some_and(|arr| arr.iter().any(|b| b.get("type").unwrap_or(&NULL) == "tool_result"))
        });
        let curr_has_tool_result = msg
            .get("content")
            .unwrap_or(&NULL)
            .as_array()
            .is_some_and(|arr| arr.iter().any(|b| b.get("type").unwrap_or(&NULL) == "tool_result"));

        if prev_has_tool_result == curr_has_tool_result {
            // Same content type — safe to merge
            let new_blocks = content_to_blocks(msg.get("content").unwrap_or(&NULL));
            if let Some(arr) = result.last_mut().and_then(|last| last.get_mut("content").and_then(Value::as_array_mut))
            {
                arr.extend(new_blocks);
            }
        } else {
            // Different content types — insert placeholder assistant to separate them
            result.push(serde_json::json!({
                "role": "assistant",
                "content": [{"type": "text", "text": "ok"}]
            }));
            let blocks = content_to_blocks(msg.get("content").unwrap_or(&NULL));
            result.push(serde_json::json!({"role": msg_role, "content": blocks}));
        }
    }

    // API requires first message to be user role. Panel injection starts with
    // assistant messages, so prepend a placeholder user message if needed.
    if result.first().is_some_and(|m| m.get("role").unwrap_or(&NULL) == "assistant") {
        result.insert(
            0,
            serde_json::json!({
                "role": "user",
                "content": [{"type": "text", "text": "ok"}]
            }),
        );
    }

    *messages = result;
}

/// Convert content (string or array) to an array of content blocks.
pub(super) fn content_to_blocks(content: &Value) -> Vec<Value> {
    if content.is_string() {
        vec![serde_json::json!({"type": "text", "text": content.as_str().unwrap_or("")})]
    } else if let Some(arr) = content.as_array() {
        arr.clone()
    } else {
        vec![]
    }
}
