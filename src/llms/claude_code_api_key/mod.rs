//! Claude Code API Key implementation.
//!
//! Uses `ANTHROPIC_API_KEY` from environment with Bearer authentication.
//! Replicates Claude Code's request signature to access Claude 4.5 models.

pub(crate) mod helpers;
pub(crate) mod streaming;

use std::sync::mpsc::Sender;

use cp_mod_utilities::secret::Redacted;
use reqwest::blocking::Client;

use super::error::LlmError;
use super::{ApiCheckResult, LlmClient, LlmRequest, StreamEvent};
use crate::infra::constants::library;
use crate::infra::tools::build_api;

use helpers::{
    BILLING_HEADER, CLAUDE_CODE_ENDPOINT, SYSTEM_REMINDER, apply_claude_code_headers, dump_last_request,
    inject_system_reminder, map_model_name,
};

/// Claude Code API Key client
pub(crate) struct ClaudeCodeApiKeyClient {
    /// Anthropic API key, resolved from vault (`"anthropic"`).
    api_key: Option<Redacted>,
}

impl ClaudeCodeApiKeyClient {
    /// Create a new client, loading the API key from the vault.
    pub(crate) fn new() -> Self {
        let api_key = Self::load_api_key();
        Self { api_key }
    }

    /// Load the API key via the credential vault.
    pub(crate) fn load_api_key() -> Option<Redacted> {
        let secret = cp_vault::vault().get("anthropic")?;
        Some(Redacted::new(secret.expose().to_owned()))
    }

    /// Run sequential API health checks: auth, streaming, and tool calling.
    pub(crate) fn check_api_impl(&self, model: &str) -> ApiCheckResult {
        let api_key = match self.api_key.as_ref() {
            Some(t) => t.expose_secret(),
            None => {
                return ApiCheckResult::failure(Some("ANTHROPIC_API_KEY not found in environment".to_owned()));
            }
        };

        let client = Client::new();
        let mapped_model = map_model_name(model);

        let system = serde_json::json!([
            {"type": "text", "text": BILLING_HEADER},
            {"type": "text", "text": "You are a helpful assistant."}
        ]);

        // Build a user message with the system-reminder marker prepended.
        let user_msg = |text: &str| {
            serde_json::json!({
                "role": "user",
                "content": [
                    {"type": "text", "text": SYSTEM_REMINDER},
                    {"type": "text", "text": text}
                ]
            })
        };

        // POST one probe body with CC headers; report whether it succeeded.
        let probe = |accept: &str, body: &serde_json::Value| {
            apply_claude_code_headers(client.post(CLAUDE_CODE_ENDPOINT), api_key, accept).json(body).send()
        };

        // Test 1: Basic auth
        let auth_result = probe(
            "application/json",
            &serde_json::json!({
                "model": mapped_model, "max_tokens": 10i32,
                "system": system, "messages": [user_msg("Hi")]
            }),
        );
        let auth_ok = auth_result.as_ref().is_ok_and(|resp| resp.status().is_success());
        if !auth_ok {
            let error = auth_result.err().map(|e| e.to_string()).or_else(|| Some("Auth failed".to_owned()));
            return ApiCheckResult::failure(error);
        }

        // Test 2: Streaming
        let streaming_ok = probe(
            "text/event-stream",
            &serde_json::json!({
                "model": mapped_model, "max_tokens": 10i32, "stream": true,
                "system": system, "messages": [user_msg("Say ok")]
            }),
        )
        .is_ok_and(|r| r.status().is_success());

        // Test 3: Tool calling
        let tools_ok = probe(
            "application/json",
            &serde_json::json!({
                "model": mapped_model, "max_tokens": 50i32, "system": system,
                "tools": [{
                    "name": "test_tool", "description": "A test tool",
                    "input_schema": {"type": "object", "properties": {}, "required": []}
                }],
                "messages": [user_msg("Hi")]
            }),
        )
        .is_ok_and(|r| r.status().is_success());

        ApiCheckResult::checks([auth_ok, streaming_ok, tools_ok])
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
            request.system_prompt.as_ref().map_or_else(|| library::default_agent_content().to_owned(), Clone::clone);

        // Build messages (api-message conversion + cleaner context + tool results)
        let super::CcJsonResult { mut json_messages, bp_hashes, bp_panel_ids, alive_count, alive_positions_permille } =
            helpers::build_cc_json_messages(&request);

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
            bp_hashes,
            bp_panel_ids,
            alive_count,
            alive_positions_permille,
        });
        Ok(())
    }

    fn check_api(&self, model: &str) -> ApiCheckResult {
        self.check_api_impl(model)
    }
}
