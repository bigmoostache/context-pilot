//! Anthropic Claude API implementation.

use reqwest::blocking::Client;
use secrecy::{ExposeSecret as _, SecretBox};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::io::{BufRead as _, BufReader};
use std::sync::mpsc::Sender;

use super::error::LlmError;
use super::{ApiMessage, ContentBlock, LlmClient, LlmRequest, StreamEvent};
use crate::infra::constants::{API_ENDPOINT, API_VERSION, library};
use crate::infra::tools::ToolUse;
use crate::infra::tools::build_api;
use cp_base::config::INJECTIONS;

pub(in crate::llms) mod messages;

use messages::{log_sse_error, messages_to_api};

/// Anthropic Claude client
pub(crate) struct AnthropicClient {
    /// API key loaded from `ANTHROPIC_API_KEY` environment variable
    api_key: Option<SecretBox<String>>,
}

impl AnthropicClient {
    /// Create a new Anthropic client, loading the API key from the environment.
    pub(crate) fn new() -> Self {
        let _r = dotenvy::dotenv().ok();
        Self { api_key: env::var("ANTHROPIC_API_KEY").ok().map(|k| SecretBox::new(Box::new(k))) }
    }
}

impl Default for AnthropicClient {
    fn default() -> Self {
        Self::new()
    }
}
/// Serializable Anthropic API request body.
#[derive(Debug, Serialize)]
struct AnthropicRequest {
    /// Model identifier
    model: String,
    /// Maximum tokens to generate
    max_tokens: u32,
    /// System prompt text
    system: String,
    /// Conversation messages
    messages: Vec<ApiMessage>,
    /// Available tools
    tools: Value,
    /// Whether to stream the response
    stream: bool,
}

/// Content block metadata from SSE stream events.
#[derive(Debug, Deserialize)]
struct StreamContentBlock {
    /// Block type (e.g. "text", "`tool_use`")
    #[serde(rename = "type")]
    block_type: Option<String>,
    /// Block ID (for `tool_use` blocks)
    id: Option<String>,
    /// Tool name (for `tool_use` blocks)
    name: Option<String>,
}

/// Delta payload from SSE stream events.
#[derive(Debug, Deserialize)]
struct StreamDelta {
    /// Delta type (e.g. "`text_delta`", "`input_json_delta`")
    #[serde(rename = "type")]
    delta_type: Option<String>,
    /// Text content delta
    text: Option<String>,
    /// Partial JSON for tool input
    partial_json: Option<String>,
    /// Stop reason (e.g. "`end_turn`", "`tool_use`")
    stop_reason: Option<String>,
}

/// Top-level SSE stream event from the Anthropic API.
#[derive(Debug, Deserialize)]
struct StreamMessage {
    /// Event type (e.g. "`content_block_start`", "`message_delta`")
    #[serde(rename = "type")]
    event_type: String,
    /// Content block index (unused but present in API)
    #[serde(default)]
    _index: Option<usize>,
    /// Content block metadata (for `block_start` events)
    content_block: Option<StreamContentBlock>,
    /// Delta payload (for delta events)
    delta: Option<StreamDelta>,
    /// Token usage statistics
    usage: Option<StreamUsage>,
}

/// Token usage statistics from the Anthropic API.
#[derive(Debug, Deserialize)]
struct StreamUsage {
    /// Number of input tokens consumed
    input_tokens: Option<usize>,
    /// Number of output tokens generated
    output_tokens: Option<usize>,
}

impl LlmClient for AnthropicClient {
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), LlmError> {
        let api_key = self.api_key.as_ref().ok_or_else(|| LlmError::Auth("ANTHROPIC_API_KEY not set".into()))?;

        // timeout(None) prevents reqwest from killing long-running SSE streams.
        // Without this, blocking Client may use system TCP timeouts, causing
        // silent stream drops mid-response (same fix applied to Claude Code providers).
        let client = Client::builder().timeout(None).build().map_err(|e| LlmError::Network(e.to_string()))?;

        // Build API messages
        let include_tool_uses = request.tool_results.is_some();
        // Use pre-assembled API messages from prompt_builder (centralized assembly)
        let mut api_messages = if request.api_messages.is_empty() {
            // Fallback: build from raw data (should only happen for api_check etc.)
            messages_to_api(
                &request.messages,
                &request.context_items,
                include_tool_uses,
                request.seed_content.as_deref(),
            )
        } else {
            request.api_messages.clone()
        };

        // Add tool results if present
        if let Some(results) = &request.tool_results {
            let tool_result_blocks: Vec<ContentBlock> = results
                .iter()
                .map(|r: &crate::infra::tools::ToolResult| ContentBlock::ToolResult {
                    tool_use_id: r.tool_use_id.clone(),
                    content: r.content.clone(),
                })
                .collect();

            api_messages.push(ApiMessage { role: "user".to_string(), content: tool_result_blocks });
        }

        // Handle cleaner mode or custom system prompt
        let system_prompt = if let Some(ref prompt) = request.system_prompt {
            if let Some(ref context) = request.extra_context {
                let msg = INJECTIONS.providers.cleaner_mode.trim_end().replace(concat!("{", "context", "}"), context);
                api_messages
                    .push(ApiMessage { role: "user".to_string(), content: vec![ContentBlock::Text { text: msg }] });
            }
            prompt.clone()
        } else {
            library::default_agent_content().to_string()
        };

        let api_request = AnthropicRequest {
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
            let path = format!("{}/{}_anthropic_last_request.json", dir, request.worker_id);
            let _r2 = std::fs::write(&path, serde_json::to_string_pretty(&api_request).unwrap_or_default());
        }

        let response = client
            .post(API_ENDPOINT)
            .header("x-api-key", api_key.expose_secret())
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&api_request)
            .send()?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().unwrap_or_default();
            return Err(LlmError::Api { status, body });
        }

        let mut reader = BufReader::new(response);
        let mut input_tokens = 0;
        let mut output_tokens = 0;
        let mut current_tool: Option<(String, String, String)> = None;
        let mut stop_reason: Option<String> = None;
        let mut total_bytes: usize = 0;
        let mut line_count: usize = 0;
        let mut last_lines: Vec<String> = Vec::new();

        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    total_bytes = total_bytes.saturating_add(n);
                    line_count = line_count.saturating_add(1);
                }
                Err(e) => {
                    let tool_ctx = current_tool.as_ref().map_or_else(
                        || "No tool in progress".to_string(),
                        |(id, name, partial)| {
                            format!("In-flight tool: {} (id={}), partial: {} bytes", name, id, partial.len())
                        },
                    );
                    let recent =
                        if last_lines.is_empty() { "(no lines read)".to_string() } else { last_lines.join("\n") };
                    return Err(LlmError::StreamRead(format!(
                        "{e}\nStream position: {total_bytes} bytes, {line_count} lines read\n{tool_ctx}\nLast SSE lines:\n{recent}"
                    )));
                }
            }
            let line = line.trim_end_matches('\n').trim_end_matches('\r');

            if !line.starts_with("data: ") {
                continue;
            }

            if last_lines.len() >= 5 {
                let _r = last_lines.remove(0);
            }
            last_lines.push(line.to_string());

            let json_str = line.get(6..).unwrap_or("");
            if json_str == "[DONE]" {
                break;
            }

            if let Ok(event) = serde_json::from_str::<StreamMessage>(json_str) {
                match event.event_type.as_str() {
                    "content_block_start" => {
                        if let Some(block) = event.content_block
                            && block.block_type.as_deref() == Some("tool_use")
                        {
                            let name = block.name.unwrap_or_default();
                            let _r =
                                tx.send(StreamEvent::ToolProgress { name: name.clone(), input_so_far: String::new() });
                            current_tool = Some((block.id.unwrap_or_default(), name, String::new()));
                        }
                    }
                    "content_block_delta" => {
                        if let Some(delta) = event.delta {
                            match delta.delta_type.as_deref() {
                                Some("text_delta") => {
                                    if let Some(text) = delta.text {
                                        let _r = tx.send(StreamEvent::Chunk(text));
                                    }
                                }
                                Some("input_json_delta") => {
                                    if let Some(json) = delta.partial_json
                                        && let Some((_, ref name, ref mut input)) = current_tool
                                    {
                                        input.push_str(&json);
                                        let _r = tx.send(StreamEvent::ToolProgress {
                                            name: name.clone(),
                                            input_so_far: input.clone(),
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    "content_block_stop" => {
                        if let Some((id, name, input_json)) = current_tool.take() {
                            let input: Value = serde_json::from_str(&input_json)
                                .unwrap_or_else(|_| Value::Object(serde_json::Map::new()));
                            let _r = tx.send(StreamEvent::ToolUse(ToolUse { id, name, input }));
                        }
                    }
                    "message_delta" => {
                        if let Some(ref delta) = event.delta
                            && let Some(ref reason) = delta.stop_reason
                        {
                            stop_reason = Some(reason.clone());
                        }
                        if let Some(usage) = event.usage {
                            if let Some(inp) = usage.input_tokens {
                                input_tokens = inp;
                            }
                            if let Some(out) = usage.output_tokens {
                                output_tokens = out;
                            }
                        }
                    }
                    "message_stop" => break,
                    "error" => {
                        log_sse_error(json_str, total_bytes, line_count, &last_lines);
                        break;
                    }
                    _ => {}
                }
            }
        }

        let _r = tx.send(StreamEvent::Done {
            input_tokens,
            output_tokens,
            cache_hit_tokens: 0,
            cache_miss_tokens: 0,
            stop_reason,
        });
        Ok(())
    }

    fn check_api(&self, model: &str) -> super::ApiCheckResult {
        let Some(api_key) = self.api_key.as_ref() else {
            return super::ApiCheckResult {
                auth_ok: false,
                streaming_ok: false,
                tools_ok: false,
                error: Some("ANTHROPIC_API_KEY not set".to_string()),
            };
        };

        let client = Client::new();
        let base = || {
            client
                .post(API_ENDPOINT)
                .header("x-api-key", api_key.expose_secret())
                .header("anthropic-version", API_VERSION)
                .header("content-type", "application/json")
        };

        // Test 1: Basic auth
        let auth_ok = base()
            .json(&serde_json::json!({
                "model": model, "max_tokens": 10,
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .send()
            .is_ok_and(|r| r.status().is_success());

        if !auth_ok {
            return super::ApiCheckResult {
                auth_ok: false,
                streaming_ok: false,
                tools_ok: false,
                error: Some("Auth failed".to_string()),
            };
        }

        // Test 2: Streaming
        let streaming_ok = base()
            .json(&serde_json::json!({
                "model": model, "max_tokens": 10, "stream": true,
                "messages": [{"role": "user", "content": "Say ok"}]
            }))
            .send()
            .is_ok_and(|r| r.status().is_success());

        // Test 3: Tools
        let tools_ok = base()
            .json(&serde_json::json!({
                "model": model, "max_tokens": 50,
                "tools": [{"name": "test_tool", "description": "A test tool",
                    "input_schema": {"type": "object", "properties": {}, "required": []}}],
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .send()
            .is_ok_and(|r| r.status().is_success());

        super::ApiCheckResult { auth_ok, streaming_ok, tools_ok, error: None }
    }
}
