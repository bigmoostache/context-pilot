//! API health check implementation for Claude Code API Key client.

use reqwest::blocking::Client;
use secrecy::ExposeSecret;

use super::ClaudeCodeApiKeyClient;
use super::helpers::{
    BILLING_HEADER, CLAUDE_CODE_ENDPOINT, SYSTEM_REMINDER, apply_claude_code_headers, map_model_name,
};
use crate::llms::ApiCheckResult;

impl ClaudeCodeApiKeyClient {
    pub(crate) fn check_api_impl(&self, model: &str) -> ApiCheckResult {
        let api_key = match self.api_key.as_ref() {
            Some(t) => t.expose_secret(),
            None => {
                return ApiCheckResult {
                    auth_ok: false,
                    streaming_ok: false,
                    tools_ok: false,
                    error: Some("ANTHROPIC_API_KEY not found in environment".to_string()),
                };
            }
        };

        let client = Client::new();
        let mapped_model = map_model_name(model);

        let system = serde_json::json!([
            {"type": "text", "text": BILLING_HEADER},
            {"type": "text", "text": "You are a helpful assistant."}
        ]);

        let user_msg = serde_json::json!({
            "role": "user",
            "content": [
                {"type": "text", "text": SYSTEM_REMINDER},
                {"type": "text", "text": "Hi"}
            ]
        });

        // Test 1: Basic auth
        let auth_result = apply_claude_code_headers(client.post(CLAUDE_CODE_ENDPOINT), api_key, "application/json")
            .json(&serde_json::json!({
                "model": mapped_model,
                "max_tokens": 10,
                "system": system,
                "messages": [user_msg]
            }))
            .send();

        #[expect(clippy::option_if_let_else, reason = "if-let is clearer here")]
        let auth_ok = match &auth_result {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        };

        if !auth_ok {
            let error = auth_result.err().map(|e| e.to_string()).or_else(|| Some("Auth failed".to_string()));
            return ApiCheckResult { auth_ok: false, streaming_ok: false, tools_ok: false, error };
        }

        // Test 2: Streaming
        let stream_msg = serde_json::json!({
            "role": "user",
            "content": [
                {"type": "text", "text": SYSTEM_REMINDER},
                {"type": "text", "text": "Say ok"}
            ]
        });
        let stream_result = apply_claude_code_headers(client.post(CLAUDE_CODE_ENDPOINT), api_key, "text/event-stream")
            .json(&serde_json::json!({
                "model": mapped_model,
                "max_tokens": 10,
                "stream": true,
                "system": system,
                "messages": [stream_msg]
            }))
            .send();

        let streaming_ok = stream_result.as_ref().map(|r| r.status().is_success()).unwrap_or(false);

        // Test 3: Tool calling
        let tools_msg = serde_json::json!({
            "role": "user",
            "content": [
                {"type": "text", "text": SYSTEM_REMINDER},
                {"type": "text", "text": "Hi"}
            ]
        });
        let tools_result = apply_claude_code_headers(client.post(CLAUDE_CODE_ENDPOINT), api_key, "application/json")
            .json(&serde_json::json!({
                "model": mapped_model,
                "max_tokens": 50,
                "system": system,
                "tools": [{
                    "name": "test_tool",
                    "description": "A test tool",
                    "input_schema": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                }],
                "messages": [tools_msg]
            }))
            .send();

        let tools_ok = tools_result.as_ref().map(|r| r.status().is_success()).unwrap_or(false);

        ApiCheckResult { auth_ok, streaming_ok, tools_ok, error: None }
    }
}
