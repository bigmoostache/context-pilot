//! Claude Code API Key implementation.
//!
//! Uses `ANTHROPIC_API_KEY` from environment with Bearer authentication.
//! Replicates Claude Code's request signature to access Claude 4.5 models.

pub(crate) mod helpers;
mod streaming;

use std::env;
use std::sync::mpsc::Sender;

use reqwest::blocking::Client;
use secrecy::{ExposeSecret as _, SecretBox};
use serde_json::Value;

use super::error::LlmError;
use super::{ApiCheckResult, LlmClient, LlmRequest, StreamEvent, api_messages_to_cc_json};
use crate::infra::constants::library;
use crate::infra::tools::build_api;
use cp_base::config::INJECTIONS;

use helpers::{
    BILLING_HEADER, CLAUDE_CODE_ENDPOINT, SYSTEM_REMINDER, apply_claude_code_headers, dump_last_request,
    ensure_message_alternation, inject_system_reminder, map_model_name,
};

/// Claude Code API Key client
pub(crate) struct ClaudeCodeApiKeyClient {
    /// Anthropic API key loaded from the `ANTHROPIC_API_KEY` environment variable
    api_key: Option<SecretBox<String>>,
}

impl ClaudeCodeApiKeyClient {
    /// Create a new client, loading the API key from environment.
    pub(crate) fn new() -> Self {
        let api_key = Self::load_api_key();
        Self { api_key }
    }

    /// Load the API key from the `ANTHROPIC_API_KEY` environment variable.
    pub(crate) fn load_api_key() -> Option<SecretBox<String>> {
        let key = env::var("ANTHROPIC_API_KEY").ok()?;
        Some(SecretBox::new(Box::new(key)))
    }

    /// Run sequential API health checks: auth, streaming, and tool calling.
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

        let auth_ok = auth_result.as_ref().is_ok_and(|resp| resp.status().is_success());

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

        let streaming_ok = stream_result.as_ref().is_ok_and(|r| r.status().is_success());

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

        let tools_ok = tools_result.as_ref().is_ok_and(|r| r.status().is_success());

        ApiCheckResult { auth_ok, streaming_ok, tools_ok, error: None }
    }
}

impl Default for ClaudeCodeApiKeyClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmClient for ClaudeCodeApiKeyClient {
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), LlmError> {
        let api_key =
            self.api_key.as_ref().ok_or_else(|| LlmError::Auth("ANTHROPIC_API_KEY not found in environment".into()))?;

        let client = Client::builder().timeout(None).build().map_err(|e| LlmError::Network(e.to_string()))?;

        // Handle cleaner mode or custom system prompt
        let system_text =
            request.system_prompt.as_ref().map_or_else(|| library::default_agent_content().to_string(), Clone::clone);

        // Build messages from pre-assembled API messages or raw data
        let mut json_messages =
            if request.api_messages.is_empty() { Vec::new() } else { api_messages_to_cc_json(&request.api_messages) };

        // Handle cleaner mode extra context
        if let Some(ref context) = request.extra_context {
            let msg = INJECTIONS.providers.cleaner_mode.trim_end().replace(concat!("{", "context", "}"), context);
            json_messages.push(serde_json::json!({
                "role": "user",
                "content": msg
            }));
        }

        // Add pending tool results
        if let Some(results) = &request.tool_results {
            let tool_results: Vec<Value> = results
                .iter()
                .map(|r: &crate::infra::tools::ToolResult| {
                    serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": r.tool_use_id,
                        "content": r.content
                    })
                })
                .collect();
            json_messages.push(serde_json::json!({
                "role": "user",
                "content": tool_results
            }));
        }

        ensure_message_alternation(&mut json_messages);
        inject_system_reminder(&mut json_messages);

        let api_request = serde_json::json!({
            "model": map_model_name(&request.model),
            "max_tokens": request.max_output_tokens,
            "system": [
                {"type": "text", "text": BILLING_HEADER},
                {"type": "text", "text": system_text}
            ],
            "messages": json_messages,
            "tools": build_api(&request.tools),
            "stream": true
        });

        dump_last_request(&request.worker_id, &api_request);

        let response =
            apply_claude_code_headers(client.post(CLAUDE_CODE_ENDPOINT), api_key.expose_secret(), "text/event-stream")
                .json(&api_request)
                .send()?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().unwrap_or_default();
            return Err(LlmError::Api { status, body });
        }

        let resp_headers: String = response
            .headers()
            .iter()
            .map(|(k, v)| format!("  {}: {}", k, v.to_str().unwrap_or("<binary>")))
            .collect::<Vec<_>>()
            .join("\n");

        let (input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, stop_reason) =
            streaming::parse_sse_stream(response, &resp_headers, &tx)?;

        let _r = tx.send(StreamEvent::Done {
            input_tokens,
            output_tokens,
            cache_hit_tokens,
            cache_miss_tokens,
            stop_reason,
        });
        Ok(())
    }

    fn check_api(&self, model: &str) -> ApiCheckResult {
        self.check_api_impl(model)
    }
}
