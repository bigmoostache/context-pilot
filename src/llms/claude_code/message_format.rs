//! Message preparation for Claude Code's API.
//!
//! Handles user/assistant alternation, system-reminder injection, and
//! content-block normalization required by the Claude Code server.

use serde_json::Value;

use super::SYSTEM_REMINDER;

/// Inject the system-reminder text block into the first non-tool-result user message.
/// Claude Code's server validates that messages contain this marker.
/// Must skip `tool_result` user messages (from panel injection) since mixing text blocks
/// into `tool_result` messages breaks the API's `tool_use/tool_result` pairing.
pub(super) fn inject_system_reminder(messages: &mut Vec<Value>) {
    let reminder = serde_json::json!({"type": "text", "text": SYSTEM_REMINDER});

    for msg in messages.iter_mut() {
        if msg["role"] != "user" {
            continue;
        }

        // Skip tool_result messages (from panel injection / tool loop)
        if let Some(arr) = msg["content"].as_array()
            && arr.iter().any(|block| block["type"] == "tool_result")
        {
            continue;
        }

        // Convert string content to array format and prepend reminder
        let content = &msg["content"];
        if content.is_string() {
            let text = content.as_str().unwrap_or("").to_string();
            msg["content"] = serde_json::json!([
                reminder,
                {"type": "text", "text": text}
            ]);
        } else if content.is_array()
            && let Some(arr) = msg["content"].as_array_mut()
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
        let same_role = result.last().is_some_and(|last: &Value| last["role"] == msg["role"]);
        if !same_role {
            let blocks = content_to_blocks(&msg["content"]);
            result.push(serde_json::json!({"role": msg["role"], "content": blocks}));
            continue;
        }

        let prev_has_tool_result = result.last().is_some_and(|last| {
            last["content"].as_array().is_some_and(|arr| arr.iter().any(|b| b["type"] == "tool_result"))
        });
        let curr_has_tool_result =
            msg["content"].as_array().is_some_and(|arr| arr.iter().any(|b| b["type"] == "tool_result"));

        if prev_has_tool_result == curr_has_tool_result {
            // Same content type — safe to merge
            let new_blocks = content_to_blocks(&msg["content"]);
            if let Some(arr) = result.last_mut().and_then(|last| last["content"].as_array_mut()) {
                arr.extend(new_blocks);
            }
        } else {
            // Different content types — insert placeholder assistant to separate them
            result.push(serde_json::json!({
                "role": "assistant",
                "content": [{"type": "text", "text": "ok"}]
            }));
            let blocks = content_to_blocks(&msg["content"]);
            result.push(serde_json::json!({"role": msg["role"], "content": blocks}));
        }
    }

    // API requires first message to be user role. Panel injection starts with
    // assistant messages, so prepend a placeholder user message if needed.
    if result.first().is_some_and(|m| m["role"] == "assistant") {
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
