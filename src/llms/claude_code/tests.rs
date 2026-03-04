use super::*;
use crate::infra::constants::API_VERSION;

/// Minimal request matching working Python exactly.
/// No panels, no tools, no message prefixes — just raw API call.
#[test]
#[ignore] // Requires OAuth token — run with `cargo test -- --ignored`
fn test_general_kenobi() {
    let token = ClaudeCodeClient::load_oauth_token().expect("OAuth token not found or expired — run 'claude login'");

    let client = reqwest::blocking::Client::new();

    // Exact same payload structure as working Python create_payload()
    let body = serde_json::json!({
        "model": "claude-opus-4-6",
        "max_tokens": 100,
        "system": [
            {"type": "text", "text": BILLING_HEADER},
            {"type": "text", "text": "You are a helpful assistant."}
        ],
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": SYSTEM_REMINDER},
                {"type": "text", "text": "Hello There! (this is a test, answer General Kenobi)"}
            ]
        }]
    });

    // Exact same headers as working Python get_claude_code_headers()
    let response = client
        .post(CLAUDE_CODE_ENDPOINT)
        .header("accept", "application/json")
        .header("authorization", format!("Bearer {}", secrecy::ExposeSecret::expose_secret(&token)))
        .header("anthropic-version", API_VERSION)
        .header("anthropic-beta", OAUTH_BETA_HEADER)
        .header("anthropic-dangerous-direct-browser-access", "true")
        .header("content-type", "application/json")
        .header("user-agent", "claude-cli/2.1.37 (external, cli)")
        .header("x-app", "cli")
        .header("x-stainless-arch", "x64")
        .header("x-stainless-lang", "js")
        .header("x-stainless-os", "Linux")
        .header("x-stainless-package-version", "0.70.0")
        .header("x-stainless-retry-count", "0")
        .header("x-stainless-runtime", "node")
        .header("x-stainless-runtime-version", "v24.3.0")
        .json(&body)
        .send()
        .expect("HTTP request failed");

    let status = response.status();
    let resp_body: serde_json::Value = response.json().expect("Failed to parse JSON response");

    assert!(status.is_success(), "API returned {}: {}", status, serde_json::to_string_pretty(&resp_body).unwrap());

    let text = resp_body["content"][0]["text"].as_str().expect("No text in response content");

    assert!(text.to_lowercase().contains("general kenobi"), "Expected 'General Kenobi' in response, got: {}", text);
}

/// Same as above but with tools and streaming — matches what stream() actually sends.
#[test]
#[ignore] // Requires OAuth token — run with `cargo test -- --ignored`
fn test_general_kenobi_with_tools_streaming() {
    let token = ClaudeCodeClient::load_oauth_token().expect("OAuth token not found or expired — run 'claude login'");

    let client = reqwest::blocking::Client::new();

    // Mimic the stream() method: tools array, streaming, max_tokens=4096
    let body = serde_json::json!({
        "model": "claude-opus-4-6",
        "max_tokens": 4096,
        "system": [
            {"type": "text", "text": BILLING_HEADER},
            {"type": "text", "text": "You are a helpful assistant."}
        ],
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": SYSTEM_REMINDER},
                {"type": "text", "text": "Hello There! (this is a test, answer General Kenobi)"}
            ]
        }],
        "tools": [{
            "name": "test_tool",
            "description": "A test tool",
            "input_schema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }],
        "stream": true
    });

    let response = client
        .post(CLAUDE_CODE_ENDPOINT)
        .header("accept", "text/event-stream")
        .header("authorization", format!("Bearer {}", secrecy::ExposeSecret::expose_secret(&token)))
        .header("anthropic-version", API_VERSION)
        .header("anthropic-beta", OAUTH_BETA_HEADER)
        .header("anthropic-dangerous-direct-browser-access", "true")
        .header("content-type", "application/json")
        .header("user-agent", "claude-cli/2.1.37 (external, cli)")
        .header("x-app", "cli")
        .header("x-stainless-arch", "x64")
        .header("x-stainless-lang", "js")
        .header("x-stainless-os", "Linux")
        .header("x-stainless-package-version", "0.70.0")
        .header("x-stainless-retry-count", "0")
        .header("x-stainless-runtime", "node")
        .header("x-stainless-runtime-version", "v24.3.0")
        .json(&body)
        .send()
        .expect("HTTP request failed");

    let status = response.status();

    // For streaming, read SSE lines and collect text
    assert!(status.is_success(), "API returned {}", status);

    let mut full_text = String::new();
    let reader = std::io::BufReader::new(response);
    for line in std::io::BufRead::lines(reader) {
        let line = line.expect("Read error");
        if !line.starts_with("data: ") {
            continue;
        }
        let json_str = &line[6..];
        if json_str == "[DONE]" {
            break;
        }
        if let Ok(event) = serde_json::from_str::<serde_json::Value>(json_str)
            && event["type"] == "content_block_delta"
            && let Some(text) = event["delta"]["text"].as_str()
        {
            full_text.push_str(text);
        }
    }

    assert!(
        full_text.to_lowercase().contains("general kenobi"),
        "Expected 'General Kenobi' in streamed response, got: {}",
        full_text
    );
}

/// Test inject_system_reminder: verify it skips tool_result messages
/// and injects into the first regular user message.
#[test]
fn test_inject_system_reminder_skips_tool_results() {
    // Simulate panel injection: tool_result user messages first, then a regular user msg
    let mut messages = vec![
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "panel"}, {"type": "tool_use", "id": "panel_P2", "name": "dynamic_panel", "input": {}}]
        }),
        serde_json::json!({
            "role": "user",
            "content": [{"type": "tool_result", "tool_use_id": "panel_P2", "content": "data"}]
        }),
        serde_json::json!({
            "role": "user",
            "content": "Hello there"
        }),
    ];

    inject_system_reminder(&mut messages);

    // tool_result message should be untouched
    assert!(
        messages[1]["content"][0]["type"] == "tool_result",
        "tool_result message was modified: {:?}",
        messages[1]["content"]
    );

    // Regular user message should now be an array with reminder first
    assert!(messages[2]["content"].is_array(), "Regular user message not converted to array");
    let arr = messages[2]["content"].as_array().unwrap();
    assert_eq!(arr.len(), 2, "Expected 2 blocks (reminder + text)");
    assert!(arr[0]["text"].as_str().unwrap().contains("system-reminder"), "First block should be system-reminder");
    assert_eq!(arr[1]["text"].as_str().unwrap(), "Hello there");
}

/// Test inject_system_reminder: when no eligible message, prepends fallback pair
#[test]
fn test_inject_system_reminder_no_eligible() {
    // Only tool_result user messages — triggers fallback
    let mut messages = vec![
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "tool_use", "id": "t1", "name": "x", "input": {}}]
        }),
        serde_json::json!({
            "role": "user",
            "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "data"}]
        }),
    ];

    inject_system_reminder(&mut messages);

    // Should have 4 messages: [fallback_user, fallback_assistant, original_assistant, original_user]
    assert_eq!(messages.len(), 4);
    // First message is the fallback user with reminder
    assert_eq!(messages[0]["role"], "user");
    assert!(messages[0]["content"][0]["text"].as_str().unwrap().contains("system-reminder"));
    // Second message is the assistant ack
    assert_eq!(messages[1]["role"], "assistant");
    // Original messages preserved at indices 2 and 3
    assert!(messages[3]["content"][0]["type"] == "tool_result");
}

/// Test ensure_message_alternation: merges consecutive text user messages,
/// but separates tool_result user from text user with a placeholder assistant.
#[test]
fn test_ensure_message_alternation() {
    // Simulate the actual failure scenario: panel footer (tool_result user)
    // followed by consecutive text user messages
    let mut messages = vec![
        serde_json::json!({"role": "assistant", "content": [{"type": "text", "text": "panel"}]}),
        serde_json::json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "panel_footer", "content": "ok"}]}),
        // These 3 consecutive text user messages should be merged, with a
        // placeholder assistant separating them from the tool_result above
        serde_json::json!({"role": "user", "content": "Hello"}),
        serde_json::json!({"role": "user", "content": "World"}),
        serde_json::json!({"role": "user", "content": "Again"}),
    ];

    ensure_message_alternation(&mut messages);

    // Should have 5: prepended user "ok", assistant, tool_result user, placeholder assistant, merged text user
    assert_eq!(
        messages.len(),
        5,
        "Got {} messages: {:?}",
        messages.len(),
        messages.iter().map(|m| m["role"].as_str().unwrap_or("?")).collect::<Vec<_>>()
    );
    assert_eq!(messages[0]["role"], "user"); // prepended because first msg was assistant
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(messages[2]["content"][0]["type"], "tool_result");
    assert_eq!(messages[3]["role"], "assistant"); // placeholder
    assert_eq!(messages[4]["role"], "user");

    // The merged text user message has all 3 texts
    let user_content = messages[4]["content"].as_array().unwrap();
    assert_eq!(user_content.len(), 3);
    assert_eq!(user_content[0]["text"], "Hello");
    assert_eq!(user_content[1]["text"], "World");
    assert_eq!(user_content[2]["text"], "Again");
}

/// Test that alternation + reminder injection work together on the real scenario
#[test]
fn test_alternation_then_reminder() {
    let mut messages = vec![
        serde_json::json!({"role": "assistant", "content": [{"type": "text", "text": "footer"}, {"type": "tool_use", "id": "panel_footer", "name": "dynamic_panel", "input": {}}]}),
        serde_json::json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "panel_footer", "content": "ok"}]}),
        serde_json::json!({"role": "user", "content": "Hello"}),
        serde_json::json!({"role": "user", "content": "World"}),
    ];

    ensure_message_alternation(&mut messages);
    inject_system_reminder(&mut messages);

    // 5 messages: prepended user (gets reminder), assistant, tool_result user, placeholder assistant, merged text user
    assert_eq!(messages.len(), 5);
    // The prepended user message (index 0) should have system-reminder injected
    let user_content = messages[0]["content"].as_array().unwrap();
    assert!(
        user_content[0]["text"].as_str().unwrap().contains("system-reminder"),
        "First block should be system-reminder, got: {:?}",
        user_content[0]
    );
}
