//! Helper functions for Claude Code API Key authentication and message processing.

use serde_json::Value;

/// System reminder injected into first user message for Claude Code validation
pub(crate) const SYSTEM_REMINDER: &str =
    "<system-reminder>\nThe following skills are available for use with the Skill tool:\n</system-reminder>";

/// API endpoint with beta flag required for Claude 4.5 access
pub(crate) const CLAUDE_CODE_ENDPOINT: &str = "https://api.anthropic.com/v1/messages?beta=true";

/// Beta header with all required flags for Claude Code access (API key mode)
pub(crate) const OAUTH_BETA_HEADER: &str = "interleaved-thinking-2025-05-14,context-management-2025-06-27,prompt-caching-scope-2026-01-05,structured-outputs-2025-12-15";

/// Billing header that must be included in system prompt
pub(crate) const BILLING_HEADER: &str =
    "x-anthropic-billing-header: cc_version=2.1.44.fbe; cc_entrypoint=cli; cch=e5401;";

/// Directory for last-request debug dumps
pub(crate) const LAST_REQUESTS_DIR: &str = ".context-pilot/last_requests";

/// Sentinel value returned by `.get()` when a key is missing.
const NULL: Value = Value::Null;

/// Map model names to full API model identifiers
pub(crate) fn map_model_name(model: &str) -> &str {
    match model {
        "claude-opus-4-6" | "claude-opus-4-5" => "claude-opus-4-6",
        "claude-sonnet-4-5" => "claude-sonnet-4-5-20250929",
        "claude-haiku-4-5" => "claude-haiku-4-5-20251001",
        _ => model,
    }
}

/// Inject the system-reminder text block into the first non-tool-result user message.
/// Claude Code's server validates that messages contain this marker.
/// Must skip `tool_result` user messages (from panel injection) since mixing text blocks
/// into `tool_result` messages breaks the API's `tool_use/tool_result` pairing.
pub(crate) fn inject_system_reminder(messages: &mut Vec<Value>) {
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
pub(crate) fn ensure_message_alternation(messages: &mut Vec<Value>) {
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
pub(crate) fn content_to_blocks(content: &Value) -> Vec<Value> {
    if content.is_string() {
        vec![serde_json::json!({"type": "text", "text": content.as_str().unwrap_or("")})]
    } else if let Some(arr) = content.as_array() {
        arr.clone()
    } else {
        vec![]
    }
}

/// Dump the outgoing API request to disk for debugging.
/// Written to `.context-pilot/last_requests/{worker_id}_last_request.json`.
pub(crate) fn dump_last_request(worker_id: &str, api_request: &Value) {
    let debug = serde_json::json!({
        "request_url": CLAUDE_CODE_ENDPOINT,
        "request_headers": {
            "anthropic-beta": OAUTH_BETA_HEADER,
            "anthropic-version": crate::infra::constants::API_VERSION,
            "user-agent": "claude-cli/2.1.44 (external, cli)",
            "x-app": "cli",
        },
        "request_body": api_request,
    });
    let _r1 = std::fs::create_dir_all(LAST_REQUESTS_DIR);
    let path = format!("{LAST_REQUESTS_DIR}/{worker_id}_last_request.json");
    let _r2 = std::fs::write(path, serde_json::to_string_pretty(&debug).unwrap_or_default());
}

/// Apply standard Claude Code request headers to a reqwest builder.
pub(crate) fn apply_claude_code_headers(
    builder: reqwest::blocking::RequestBuilder,
    api_key: &str,
    accept: &str,
) -> reqwest::blocking::RequestBuilder {
    builder
        .header("accept", accept)
        .header("x-api-key", api_key)
        .header("anthropic-version", crate::infra::constants::API_VERSION)
        .header("anthropic-beta", OAUTH_BETA_HEADER)
        .header("anthropic-dangerous-direct-browser-access", "true")
        .header("content-type", "application/json")
        .header("user-agent", "claude-cli/2.1.44 (external, cli)")
        .header("x-app", "cli")
        .header("x-stainless-arch", "x64")
        .header("x-stainless-lang", "js")
        .header("x-stainless-os", "Linux")
        .header("x-stainless-package-version", "0.74.0")
        .header("x-stainless-timeout", "600")
        .header("x-stainless-retry-count", "0")
        .header("x-stainless-runtime", "node")
        .header("x-stainless-runtime-version", "v24.3.0")
}
