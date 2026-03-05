//! API health-check for Claude Code OAuth.
//!
//! Three sequential checks: auth → streaming → tool calling.

use reqwest::blocking::Client;
use secrecy::ExposeSecret as _;
use serde_json::Value;

use super::{
    ApiCheckResult, BILLING_HEADER, CLAUDE_CODE_ENDPOINT, ClaudeCodeClient, OAUTH_BETA_HEADER, SYSTEM_REMINDER,
    map_model_name,
};
use crate::infra::constants::API_VERSION;

impl ClaudeCodeClient {
    pub(crate) fn do_check_api(&self, model: &str) -> ApiCheckResult {
        let access_token = match self.access_token.as_ref() {
            Some(t) => t.expose_secret(),
            None => {
                return ApiCheckResult {
                    auth_ok: false,
                    streaming_ok: false,
                    tools_ok: false,
                    error: Some("OAuth token not found or expired".to_string()),
                };
            }
        };

        let client = Client::new();
        let mapped_model = map_model_name(model);
        let system = system_block();

        // Test 1: Basic auth — simple non-streaming request
        let auth_result = build_check_request(&client, access_token, mapped_model, &system, "Hi", false, None).send();
        let auth_ok = auth_result.as_ref().is_ok_and(|r| r.status().is_success());
        if !auth_ok {
            let error = auth_result.err().map(|e| e.to_string()).or_else(|| Some("Auth failed".to_string()));
            return ApiCheckResult { auth_ok: false, streaming_ok: false, tools_ok: false, error };
        }

        // Test 2: Streaming
        let stream_result =
            build_check_request(&client, access_token, mapped_model, &system, "Say ok", true, None).send();
        let streaming_ok = stream_result.as_ref().is_ok_and(|r| r.status().is_success());

        // Test 3: Tool calling
        let test_tool = serde_json::json!([{
            "name": "test_tool",
            "description": "A test tool",
            "input_schema": {"type": "object", "properties": {}, "required": []}
        }]);
        let tools_result =
            build_check_request(&client, access_token, mapped_model, &system, "Hi", false, Some(&test_tool)).send();
        let tools_ok = tools_result.as_ref().is_ok_and(|r| r.status().is_success());

        ApiCheckResult { auth_ok, streaming_ok, tools_ok, error: None }
    }
}

/// System block with billing header required by Claude Code.
fn system_block() -> Value {
    serde_json::json!([
        {"type": "text", "text": BILLING_HEADER},
        {"type": "text", "text": "You are a helpful assistant."}
    ])
}

/// Build a health-check request with standard Claude Code headers.
fn build_check_request(
    client: &Client,
    access_token: &str,
    model: &str,
    system: &Value,
    user_text: &str,
    stream: bool,
    tools: Option<&Value>,
) -> reqwest::blocking::RequestBuilder {
    let user_msg = serde_json::json!({
        "role": "user",
        "content": [
            {"type": "text", "text": SYSTEM_REMINDER},
            {"type": "text", "text": user_text}
        ]
    });

    let max_tokens = tools.map_or(10, |_| 50);
    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": max_tokens,
        "system": system,
        "messages": [user_msg]
    });
    if stream {
        body["stream"] = serde_json::json!(true);
    }
    if let Some(t) = tools {
        body["tools"] = t.clone();
    }

    let accept = if stream { "text/event-stream" } else { "application/json" };

    client
        .post(CLAUDE_CODE_ENDPOINT)
        .header("accept", accept)
        .header("authorization", format!("Bearer {access_token}"))
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
}
