//! `DeepSeek` API implementation.
//!
//! `DeepSeek` uses an OpenAI-compatible API format.
//! Message building is delegated to the shared `openai_compat` module,
//! with a thin wrapper to add `reasoning_content` for deepseek-reasoner.

use std::env;
use std::io::{BufRead as _, BufReader};
use std::sync::mpsc::Sender;

use reqwest::blocking::Client;
use secrecy::{ExposeSecret as _, SecretBox};
use serde::Serialize;

use super::super::error::LlmError;
use super::super::openai_compat::{self, BuildOptions, OaiMessage};
use super::super::openai_streaming::ToolCallAccumulator;
use super::super::{LlmClient, LlmRequest, StreamEvent};

/// `DeepSeek` chat completions API endpoint.
const DEEPSEEK_API_ENDPOINT: &str = "https://api.deepseek.com/chat/completions";

/// `DeepSeek` client
pub(crate) struct DeepSeekClient {
    /// API key loaded from the `DEEPSEEK_API_KEY` environment variable.
    api_key: Option<SecretBox<String>>,
}

impl DeepSeekClient {
    /// Create a new `DeepSeekClient`, reading the API key from the environment.
    pub(crate) fn new() -> Self {
        let _r = dotenvy::dotenv().ok();
        Self { api_key: env::var("DEEPSEEK_API_KEY").ok().map(|k| SecretBox::new(Box::new(k))) }
    }
}

impl Default for DeepSeekClient {
    fn default() -> Self {
        Self::new()
    }
}

// ───────────────────────────────────────────────────────────────────
// DeepSeek-specific message type (adds reasoning_content field)
// ───────────────────────────────────────────────────────────────────

/// `DeepSeek` message — wraps the shared `OaiMessage` but adds `reasoning_content`
/// which is required for deepseek-reasoner model on assistant messages.
#[derive(Debug, Serialize)]
struct DsMessage {
    /// Message role (`"system"`, `"user"`, `"assistant"`, or `"tool"`).
    role: String,
    /// Text content of the message.
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    /// Chain-of-thought reasoning (deepseek-reasoner only).
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
    /// Tool calls made by the assistant.
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<openai_compat::OaiToolCall>>,
    /// ID of the tool call this message is a result for.
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

impl DsMessage {
    /// Convert from shared `OaiMessage`, adding `reasoning_content` for assistant messages.
    fn from_oai(msg: OaiMessage, is_reasoner: bool) -> Self {
        let reasoning_content = (is_reasoner && msg.role == "assistant").then(String::new);
        Self {
            role: msg.role,
            content: msg.content,
            reasoning_content,
            tool_calls: msg.tool_calls,
            tool_call_id: msg.tool_call_id,
        }
    }
}

/// Serializable request body for the `DeepSeek` chat completions API.
#[derive(Debug, Serialize)]
struct DsRequest {
    /// Model identifier (e.g. `"deepseek-chat"`, `"deepseek-reasoner"`).
    model: String,
    /// Conversation messages.
    messages: Vec<DsMessage>,
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

impl LlmClient for DeepSeekClient {
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), LlmError> {
        let api_key = self.api_key.as_ref().ok_or_else(|| LlmError::Auth("DEEPSEEK_API_KEY not set".into()))?;

        let client = Client::new();
        let is_reasoner = request.model == "deepseek-reasoner";

        // Collect pending tool result IDs
        let pending_tool_ids: Vec<String> = request
            .tool_results
            .as_ref()
            .map(|results: &Vec<crate::infra::tools::ToolResult>| {
                results.iter().map(|r| r.tool_use_id.clone()).collect()
            })
            .unwrap_or_default();

        // Build messages using shared builder
        let oai_messages = openai_compat::build_messages(
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

        // Convert to DeepSeek format (adds reasoning_content for assistant messages)
        let mut ds_messages: Vec<DsMessage> =
            oai_messages.into_iter().map(|m| DsMessage::from_oai(m, is_reasoner)).collect();

        // Add tool results if present
        if let Some(results) = &request.tool_results {
            for result in results {
                ds_messages.push(DsMessage {
                    role: "tool".to_string(),
                    content: Some(result.content.clone()),
                    reasoning_content: None,
                    tool_calls: None,
                    tool_call_id: Some(result.tool_use_id.clone()),
                });
            }
        }

        let tools = openai_compat::tools_to_oai(&request.tools);
        let tool_choice = if tools.is_empty() { None } else { Some("auto".to_string()) };

        let api_request = DsRequest {
            model: request.model.clone(),
            messages: ds_messages,
            tools,
            tool_choice,
            max_tokens: request.max_output_tokens,
            stream: true,
        };

        super::super::openai_streaming::dump_request(&request.worker_id, "deepseek", &api_request);

        let response = client
            .post(DEEPSEEK_API_ENDPOINT)
            .header("Authorization", format!("Bearer {}", api_key.expose_secret()))
            .header("Content-Type", "application/json")
            .json(&api_request)
            .send()?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().unwrap_or_default();
            return Err(LlmError::Api { status, body });
        }

        // Stream SSE using shared helpers
        let reader = BufReader::new(response);
        let mut input_tokens = 0;
        let mut output_tokens = 0;
        let mut cache_hit_tokens = 0;
        let mut cache_miss_tokens = 0;
        let mut stop_reason: Option<String> = None;
        let mut tool_acc = ToolCallAccumulator::new();

        for line in reader.lines() {
            let line = line.map_err(|e| LlmError::StreamRead(e.to_string()))?;

            if let Some(resp) = super::super::openai_streaming::parse_sse_line(&line) {
                if let Some(usage) = resp.usage {
                    if let Some(inp) = usage.prompt {
                        input_tokens = inp;
                    }
                    if let Some(out) = usage.completion {
                        output_tokens = out;
                    }
                    // DeepSeek-specific cache fields
                    if let Some(hit) = usage.prompt_cache_hit {
                        cache_hit_tokens = hit;
                    }
                    if let Some(miss) = usage.prompt_cache_miss {
                        cache_miss_tokens = miss;
                    }
                }

                for choice in resp.choices {
                    if let Some(delta) = choice.delta {
                        if let Some(content) = delta.content
                            && !content.is_empty()
                        {
                            let _r = tx.send(StreamEvent::Chunk(content));
                        }
                        if let Some(calls) = delta.tool_calls {
                            for call in &calls {
                                if let Some((name, input_so_far)) = tool_acc.feed(call) {
                                    let _r = tx.send(StreamEvent::ToolProgress { name, input_so_far });
                                }
                            }
                        }
                    }
                    if let Some(ref reason) = choice.finish_reason {
                        stop_reason = Some(super::super::openai_streaming::normalize_stop_reason(reason));
                        for tool_use in tool_acc.drain() {
                            let _r = tx.send(StreamEvent::ToolUse(tool_use));
                        }
                    }
                }
            }
        }

        let _r = tx.send(StreamEvent::Done {
            input_tokens,
            output_tokens,
            cache_hit_tokens,
            cache_miss_tokens,
            stop_reason,
        });
        Ok(())
    }

    fn check_api(&self, model: &str) -> super::super::ApiCheckResult {
        let Some(api_key) = self.api_key.as_ref() else {
            return super::super::ApiCheckResult {
                auth_ok: false,
                streaming_ok: false,
                tools_ok: false,
                error: Some("DEEPSEEK_API_KEY not set".to_string()),
            };
        };

        let client = Client::new();

        // Test 1: Basic auth
        let auth_result = client
            .post(DEEPSEEK_API_ENDPOINT)
            .header("Authorization", format!("Bearer {}", api_key.expose_secret()))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "max_tokens": 10,
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .send();

        let auth_ok = auth_result.as_ref().is_ok_and(|r| r.status().is_success());

        if !auth_ok {
            let error = auth_result.err().map(|e| e.to_string()).or_else(|| Some("Auth failed".to_string()));
            return super::super::ApiCheckResult { auth_ok: false, streaming_ok: false, tools_ok: false, error };
        }

        // Test 2: Streaming
        let stream_result = client
            .post(DEEPSEEK_API_ENDPOINT)
            .header("Authorization", format!("Bearer {}", api_key.expose_secret()))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "max_tokens": 10,
                "stream": true,
                "messages": [{"role": "user", "content": "Say ok"}]
            }))
            .send();

        let streaming_ok = stream_result.as_ref().is_ok_and(|r| r.status().is_success());

        // Test 3: Tools
        let tools_result = client
            .post(DEEPSEEK_API_ENDPOINT)
            .header("Authorization", format!("Bearer {}", api_key.expose_secret()))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "max_tokens": 50,
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
