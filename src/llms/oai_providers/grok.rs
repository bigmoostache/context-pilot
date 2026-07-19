//! xAI Grok API implementation.
//!
//! Grok uses an OpenAI-compatible API format.
//! Message building is delegated to the shared `openai_compat` module.

use std::io::BufReader;
use std::sync::mpsc::Sender;

use cp_mod_utilities::secret::Redacted;
use reqwest::blocking::Client;
use serde::Serialize;

use super::super::error::LlmError;
use super::super::{LlmClient, LlmRequest, StreamEvent};
use super::openai_compat::{self, BuildOptions, OaiMessage};
use super::openai_streaming::ToolCallAccumulator;

/// xAI Grok chat completions API endpoint.
const GROK_API_ENDPOINT: &str = "https://api.x.ai/v1/chat/completions";

/// xAI Grok client
pub(crate) struct GrokClient {
    /// xAI API key, resolved from vault (`"xai"`).
    api_key: Option<Redacted>,
}

impl GrokClient {
    /// Create a new `GrokClient`, reading the API key from the vault.
    pub(crate) fn new() -> Self {
        let _r = dotenvy::dotenv().ok();
        Self { api_key: cp_vault::vault().get("xai").map(|s| Redacted::new(s.expose().to_owned())) }
    }
}

impl Default for GrokClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Serializable request body for the Grok chat completions API.
#[derive(Debug, Serialize)]
struct GrokRequest {
    /// Model identifier (e.g. `"grok-3"`).
    model: String,
    /// Conversation messages.
    messages: Vec<OaiMessage>,
    /// Tool definitions available to the model.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<openai_compat::OaiTool>,
    /// Tool selection strategy (e.g. `"auto"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    /// Maximum number of tokens to generate.
    max_tokens: u32,
    /// Whether to stream the response via SSE.
    stream: bool,
}

impl LlmClient for GrokClient {
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), LlmError> {
        let api_key = self.api_key.as_ref().ok_or_else(|| LlmError::Auth("XAI_API_KEY not set".into()))?;

        let client = Client::new();

        // Collect pending tool result IDs
        let pending_tool_ids: Vec<String> = request
            .tool_results
            .as_ref()
            .map(|results: &Vec<crate::infra::tools::ToolResult>| {
                results.iter().map(|r| r.tool_use_id.clone()).collect()
            })
            .unwrap_or_default();

        // Build messages using shared builder
        let mut messages = openai_compat::build_messages(
            &request.messages,
            &request.context_items,
            &BuildOptions {
                system_prompt: request.system_prompt.clone(),
                system_suffix: None,
                extra_context: request.extra_context.clone(),
                pending_tool_result_ids: pending_tool_ids,
            },
            &request.api_messages,
        );

        // Add tool results if present
        if let Some(results) = &request.tool_results {
            for result in results {
                messages.push(OaiMessage {
                    role: "tool".to_owned(),
                    content: Some(result.content.clone()),
                    tool_calls: None,
                    tool_call_id: Some(result.tool_use_id.clone()),
                });
            }
        }

        let tools = openai_compat::tools_to_oai(&request.tools);
        let tool_choice = if tools.is_empty() { None } else { Some("auto".to_owned()) };

        let api_request = GrokRequest {
            model: request.model.clone(),
            messages,
            tools,
            tool_choice,
            max_tokens: request.max_output_tokens,
            stream: true,
        };

        super::openai_streaming::dump_request(&request.worker_id, "grok", &api_request);

        let response = client
            .post(GROK_API_ENDPOINT)
            .header("Authorization", format!("Bearer {}", api_key.expose_secret()))
            .header("Content-Type", "application/json")
            .json(&api_request)
            .send()?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().unwrap_or_default();
            return Err(LlmError::Api { status, body });
        }

        // Stream SSE using shared consumer
        let reader = BufReader::new(response);
        let mut tool_acc = ToolCallAccumulator::new();
        let acc = super::openai_streaming::consume_sse_stream(reader, &tx, &mut tool_acc)?;

        let _r = tx.send(StreamEvent::Done {
            input_tokens: acc.input_tokens,
            output_tokens: acc.output_tokens,
            cache_hit_tokens: 0,
            cache_miss_tokens: 0,
            stop_reason: acc.stop_reason,
            bp_hashes: vec![],
            bp_panel_ids: vec![],
            alive_count: 0,
            alive_positions_permille: vec![],
        });
        Ok(())
    }

    fn check_api(&self, model: &str) -> super::super::ApiCheckResult {
        let Some(api_key) = self.api_key.as_ref() else {
            return super::super::ApiCheckResult {
                auth_ok: false,
                streaming_ok: false,
                tools_ok: false,
                error: Some("XAI_API_KEY not set".to_owned()),
            };
        };

        let client = Client::new();

        // Test 1: Basic auth
        let auth_result = client
            .post(GROK_API_ENDPOINT)
            .header("Authorization", format!("Bearer {}", api_key.expose_secret()))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "max_tokens": 10i32,
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .send();

        let auth_ok = auth_result.as_ref().is_ok_and(|r| r.status().is_success());

        if !auth_ok {
            let error = auth_result.err().map(|e| e.to_string()).or_else(|| Some("Auth failed".to_owned()));
            return super::super::ApiCheckResult { auth_ok: false, streaming_ok: false, tools_ok: false, error };
        }

        // Test 2: Streaming
        let stream_result = client
            .post(GROK_API_ENDPOINT)
            .header("Authorization", format!("Bearer {}", api_key.expose_secret()))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "max_tokens": 10i32,
                "stream": true,
                "messages": [{"role": "user", "content": "Say ok"}]
            }))
            .send();

        let streaming_ok = stream_result.as_ref().is_ok_and(|r| r.status().is_success());

        // Test 3: Tools
        let tools_result = client
            .post(GROK_API_ENDPOINT)
            .header("Authorization", format!("Bearer {}", api_key.expose_secret()))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "max_tokens": 50i32,
                "tools": [{
                    "type": "function",
                    "function": {
                        "name": "test_tool",
                        "description": "A test tool",
                        "parameters": {
                            "type": "object",
                            "properties": {},
                            "required": []
                        }
                    }
                }],
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .send();

        let tools_ok = tools_result.as_ref().is_ok_and(|r| r.status().is_success());

        super::super::ApiCheckResult { auth_ok, streaming_ok, tools_ok, error: None }
    }
}
