//! MiniMax provider — Anthropic-compatible API via Token Plan.
//!
//! Reuses the Anthropic message format and SSE streaming protocol.
//! Endpoint: `https://api.minimax.io/anthropic/v1/messages`

use cp_mod_utilities::secret::Redacted;
use reqwest::blocking::Client;
use serde::Serialize;
use serde_json::Value;
use std::sync::mpsc::Sender;

use super::error::LlmError;
use super::{ApiMessage, ContentBlock, LlmClient, LlmRequest, StreamEvent};
use crate::infra::constants::library;
use crate::infra::tools::build_api;
use cp_base::config::INJECTIONS;

/// `MiniMax` Anthropic-compatible API endpoint.
const MINIMAX_ENDPOINT: &str = "https://api.minimax.io/anthropic/v1/messages";

/// Anthropic API version used for `MiniMax` compatibility.
const MINIMAX_API_VERSION: &str = "2023-06-01";

/// `MiniMax` client backed by the Token Plan API.
pub(crate) struct MiniMaxClient {
    /// `MiniMax` API key, resolved from vault (`"minimax"`).
    api_key: Option<Redacted>,
}

impl MiniMaxClient {
    /// Create a new `MiniMax` client, loading the API key from the vault.
    pub(crate) fn new() -> Self {
        let _r = dotenvy::dotenv().ok();
        Self { api_key: cp_vault::vault().get("minimax").map(|s| Redacted::new(s.expose().to_owned())) }
    }
}

/// Serializable `MiniMax` API request body (Anthropic-compatible format).
#[derive(Debug, Serialize)]
struct MiniMaxRequest {
    /// Model identifier.
    model: String,
    /// Maximum tokens to generate.
    max_tokens: u32,
    /// System prompt text.
    system: String,
    /// Conversation messages.
    messages: Vec<ApiMessage>,
    /// Available tools.
    tools: Value,
    /// Whether to stream the response.
    stream: bool,
}

impl LlmClient for MiniMaxClient {
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), LlmError> {
        let api_key = self.api_key.as_ref().ok_or_else(|| LlmError::Auth("MINIMAX_API_KEY not set".into()))?;

        let client = Client::builder().timeout(None).build().map_err(|e| LlmError::Network(e.to_string()))?;

        // Use pre-assembled API messages from prompt_builder
        let include_tool_uses = request.tool_results.is_some();
        let mut api_messages = if request.api_messages.is_empty() {
            super::anthropic::messages::messages_to_api(
                &request.messages,
                &request.context_items,
                include_tool_uses,
                request.seed_content.as_deref(),
            )
        } else {
            request.api_messages.clone()
        };

        // Append tool results if present
        if let Some(results) = request.tool_results.as_ref() {
            let tool_result_blocks: Vec<ContentBlock> = results
                .iter()
                .map(|r: &crate::infra::tools::ToolResult| ContentBlock::ToolResult {
                    tool_use_id: r.tool_use_id.clone(),
                    content: r.content.clone(),
                })
                .collect();
            api_messages.push(ApiMessage { role: "user".to_owned(), content: tool_result_blocks });
        }

        // Handle system prompt
        let system_prompt = if let Some(prompt) = request.system_prompt.as_ref() {
            if let Some(context) = request.extra_context.as_ref() {
                let msg = INJECTIONS.providers.cleaner_mode.trim_end().replace(concat!("{", "context", "}"), context);
                api_messages
                    .push(ApiMessage { role: "user".to_owned(), content: vec![ContentBlock::Text { text: msg }] });
            }
            prompt.clone()
        } else {
            library::default_agent_content().to_owned()
        };

        let api_request = MiniMaxRequest {
            model: request.model.clone(),
            max_tokens: request.max_output_tokens,
            system: system_prompt,
            messages: api_messages,
            tools: build_api(&request.tools),
            stream: true,
        };

        // Dump last request for debugging
        {
            let dir = ".context-pilot/last_requests";
            let _r1 = std::fs::create_dir_all(dir);
            let path = format!("{dir}/{}_minimax_last_request.json", request.worker_id);
            let _r2 = std::fs::write(&path, serde_json::to_string_pretty(&api_request).unwrap_or_default());
        }

        let response = client
            .post(MINIMAX_ENDPOINT)
            .header("x-api-key", api_key.expose_secret())
            .header("anthropic-version", MINIMAX_API_VERSION)
            .header("content-type", "application/json")
            .json(&api_request)
            .send()?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().unwrap_or_default();
            return Err(LlmError::Api { status, body });
        }

        let totals = super::anthropic::streaming::consume_anthropic_stream(response, &tx, "minimax")?;

        let _r = tx.send(StreamEvent::Done {
            input_tokens: totals.input_tokens,
            output_tokens: totals.output_tokens,
            cache_hit_tokens: 0,
            cache_miss_tokens: 0,
            stop_reason: totals.stop_reason,
            bp_hashes: vec![],
            bp_panel_ids: vec![],
            alive_count: 0,
            alive_positions_permille: vec![],
        });
        Ok(())
    }

    fn check_api(&self, model: &str) -> super::ApiCheckResult {
        let Some(api_key) = self.api_key.as_ref() else {
            return super::ApiCheckResult::failure(Some("MINIMAX_API_KEY not set".to_owned()));
        };

        let client = Client::new();
        let base = || {
            client
                .post(MINIMAX_ENDPOINT)
                .header("x-api-key", api_key.expose_secret())
                .header("anthropic-version", MINIMAX_API_VERSION)
                .header("content-type", "application/json")
        };

        // Test 1: Basic auth
        let auth_ok = base()
            .json(&serde_json::json!({
                "model": model, "max_tokens": 10i32,
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .send()
            .is_ok_and(|r| r.status().is_success());

        if !auth_ok {
            return super::ApiCheckResult::failure(Some("Auth failed \u{2014} check MINIMAX_API_KEY".to_owned()));
        }

        // Test 2: Streaming
        let streaming_ok = base()
            .json(&serde_json::json!({
                "model": model, "max_tokens": 10i32, "stream": true,
                "messages": [{"role": "user", "content": "Say ok"}]
            }))
            .send()
            .is_ok_and(|r| r.status().is_success());

        // Test 3: Tools
        let tools_ok = base()
            .json(&serde_json::json!({
                "model": model, "max_tokens": 50i32,
                "tools": [{"name": "test_tool", "description": "A test tool",
                    "input_schema": {"type": "object", "properties": {}, "required": []}}],
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .send()
            .is_ok_and(|r| r.status().is_success());

        {
            let mut r = super::ApiCheckResult::default();
            r.auth_ok = auth_ok;
            r.streaming_ok = streaming_ok;
            r.tools_ok = tools_ok;
            r
        }
    }
}
