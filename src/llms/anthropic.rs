//! Anthropic Claude API implementation.

use std::env;
use std::io::{BufRead, BufReader};
use std::sync::mpsc::Sender;

use reqwest::blocking::Client;
use secrecy::{ExposeSecret, SecretBox};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::LlmError;
use super::{
    ApiMessage, ContentBlock, LlmClient, LlmRequest, StreamEvent, panel_footer_text, panel_header_text,
    panel_timestamp_text, prepare_panel_messages,
};
use crate::constants::{API_ENDPOINT, API_VERSION, MAX_RESPONSE_TOKENS, library};
use crate::core::panels::now_ms;
use crate::state::{Message, MessageStatus, MessageType};
use crate::tool_defs::build_api_tools;
use crate::tools::ToolUse;

/// Anthropic Claude client
pub struct AnthropicClient {
    api_key: Option<SecretBox<String>>,
}

impl AnthropicClient {
    pub fn new() -> Self {
        dotenvy::dotenv().ok();
        Self { api_key: env::var("ANTHROPIC_API_KEY").ok().map(|k| SecretBox::new(Box::new(k))) }
    }
}

impl Default for AnthropicClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<ApiMessage>,
    tools: Value,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct StreamContentBlock {
    #[serde(rename = "type")]
    block_type: Option<String>,
    id: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    #[serde(rename = "type")]
    delta_type: Option<String>,
    text: Option<String>,
    partial_json: Option<String>,
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamMessage {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    _index: Option<usize>,
    content_block: Option<StreamContentBlock>,
    delta: Option<StreamDelta>,
    usage: Option<StreamUsage>,
}

#[derive(Debug, Deserialize)]
struct StreamUsage {
    input_tokens: Option<usize>,
    output_tokens: Option<usize>,
}

impl LlmClient for AnthropicClient {
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), LlmError> {
        let api_key = self.api_key.as_ref().ok_or_else(|| LlmError::Auth("ANTHROPIC_API_KEY not set".into()))?;

        let client = Client::new();

        // Build API messages
        let include_tool_uses = request.tool_results.is_some();
        let mut api_messages = messages_to_api(
            &request.messages,
            &request.context_items,
            include_tool_uses,
            request.seed_content.as_deref(),
        );

        // Add tool results if present
        if let Some(results) = &request.tool_results {
            let tool_result_blocks: Vec<ContentBlock> = results
                .iter()
                .map(|r| ContentBlock::ToolResult { tool_use_id: r.tool_use_id.clone(), content: r.content.clone() })
                .collect();

            api_messages.push(ApiMessage { role: "user".to_string(), content: tool_result_blocks });
        }

        // Handle cleaner mode or custom system prompt
        let system_prompt = if let Some(ref prompt) = request.system_prompt {
            if let Some(ref context) = request.extra_context {
                api_messages.push(ApiMessage {
                    role: "user".to_string(),
                    content: vec![ContentBlock::Text {
                        text: format!("Please clean up the context to reduce token usage:\n\n{}", context),
                    }],
                });
            }
            prompt.clone()
        } else {
            library::default_agent_content().to_string()
        };

        let api_request = AnthropicRequest {
            model: request.model.clone(),
            max_tokens: MAX_RESPONSE_TOKENS,
            system: system_prompt,
            messages: api_messages,
            tools: build_api_tools(&request.tools),
            stream: true,
        };

        // Dump last request for debugging
        {
            let dir = ".context-pilot/last_requests";
            let _ = std::fs::create_dir_all(dir);
            let path = format!("{}/{}_anthropic_last_request.json", dir, request.worker_id);
            let _ = std::fs::write(&path, serde_json::to_string_pretty(&api_request).unwrap_or_default());
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

        let reader = BufReader::new(response);
        let mut input_tokens = 0;
        let mut output_tokens = 0;
        let mut current_tool: Option<(String, String, String)> = None;
        let mut stop_reason: Option<String> = None;

        for line in reader.lines() {
            let line = line.map_err(|e| LlmError::StreamRead(e.to_string()))?;

            if !line.starts_with("data: ") {
                continue;
            }

            let json_str = &line[6..];
            if json_str == "[DONE]" {
                break;
            }

            if let Ok(event) = serde_json::from_str::<StreamMessage>(json_str) {
                match event.event_type.as_str() {
                    "content_block_start" => {
                        if let Some(block) = event.content_block
                            && block.block_type.as_deref() == Some("tool_use")
                        {
                            current_tool =
                                Some((block.id.unwrap_or_default(), block.name.unwrap_or_default(), String::new()));
                        }
                    }
                    "content_block_delta" => {
                        if let Some(delta) = event.delta {
                            match delta.delta_type.as_deref() {
                                Some("text_delta") => {
                                    if let Some(text) = delta.text {
                                        let _ = tx.send(StreamEvent::Chunk(text));
                                    }
                                }
                                Some("input_json_delta") => {
                                    if let Some(json) = delta.partial_json
                                        && let Some((_, _, ref mut input)) = current_tool
                                    {
                                        input.push_str(&json);
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
                            let _ = tx.send(StreamEvent::ToolUse(ToolUse { id, name, input }));
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
                    _ => {}
                }
            }
        }

        let _ = tx.send(StreamEvent::Done {
            input_tokens,
            output_tokens,
            cache_hit_tokens: 0,
            cache_miss_tokens: 0,
            stop_reason,
        });
        Ok(())
    }

    fn check_api(&self, model: &str) -> super::ApiCheckResult {
        let api_key = match self.api_key.as_ref() {
            Some(k) => k,
            None => {
                return super::ApiCheckResult {
                    auth_ok: false,
                    streaming_ok: false,
                    tools_ok: false,
                    error: Some("ANTHROPIC_API_KEY not set".to_string()),
                };
            }
        };

        let client = Client::new();

        // Test 1: Basic auth
        let auth_result = client
            .post(API_ENDPOINT)
            .header("x-api-key", api_key.expose_secret())
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "max_tokens": 10,
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .send();

        let auth_ok = auth_result.as_ref().map(|r| r.status().is_success()).unwrap_or(false);

        if !auth_ok {
            let error = auth_result.err().map(|e| e.to_string()).or_else(|| Some("Auth failed".to_string()));
            return super::ApiCheckResult { auth_ok: false, streaming_ok: false, tools_ok: false, error };
        }

        // Test 2: Streaming
        let stream_result = client
            .post(API_ENDPOINT)
            .header("x-api-key", api_key.expose_secret())
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "max_tokens": 10,
                "stream": true,
                "messages": [{"role": "user", "content": "Say ok"}]
            }))
            .send();

        let streaming_ok = stream_result.as_ref().map(|r| r.status().is_success()).unwrap_or(false);

        // Test 3: Tools
        let tools_result = client
            .post(API_ENDPOINT)
            .header("x-api-key", api_key.expose_secret())
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "max_tokens": 50,
                "tools": [{
                    "name": "test_tool",
                    "description": "A test tool",
                    "input_schema": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                }],
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .send();

        let tools_ok = tools_result.as_ref().map(|r| r.status().is_success()).unwrap_or(false);

        super::ApiCheckResult { auth_ok, streaming_ok, tools_ok, error: None }
    }
}

/// Convert internal messages to Anthropic API format
/// Context items are injected as fake tool call/result pairs at the start
fn messages_to_api(
    messages: &[Message],
    context_items: &[crate::core::panels::ContextItem],
    include_last_tool_uses: bool,
    seed_content: Option<&str>,
) -> Vec<ApiMessage> {
    let mut api_messages: Vec<ApiMessage> = Vec::new();
    let current_ms = now_ms();

    // Inject context panels as fake tool call/result pairs (P2+ only, sorted by timestamp)
    let fake_panels = prepare_panel_messages(context_items);

    if !fake_panels.is_empty() {
        // Add header as first panel's text
        for (idx, panel) in fake_panels.iter().enumerate() {
            let timestamp_text = panel_timestamp_text(panel.timestamp_ms);
            let text = if idx == 0 {
                // First panel includes the header
                format!("{}\n\n{}", panel_header_text(), timestamp_text)
            } else {
                timestamp_text
            };

            // Assistant message with tool_use
            api_messages.push(ApiMessage {
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::Text { text },
                    ContentBlock::ToolUse {
                        id: format!("panel_{}", panel.panel_id),
                        name: "dynamic_panel".to_string(),
                        input: serde_json::json!({ "id": panel.panel_id }),
                    },
                ],
            });

            // User message with tool_result
            api_messages.push(ApiMessage {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: format!("panel_{}", panel.panel_id),
                    content: panel.content.clone(),
                }],
            });
        }

        // Add footer after all panels
        let footer = panel_footer_text(messages, current_ms);
        api_messages.push(ApiMessage {
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Text { text: footer },
                ContentBlock::ToolUse {
                    id: "panel_footer".to_string(),
                    name: "dynamic_panel".to_string(),
                    input: serde_json::json!({ "action": "end_panels" }),
                },
            ],
        });
        api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "panel_footer".to_string(),
                content: crate::constants::prompts::panel_footer_ack().to_string(),
            }],
        });

        // Re-inject seed/system prompt after panels (before conversation messages)
        if let Some(seed) = seed_content {
            api_messages.push(ApiMessage {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: format!("System instructions (repeated for emphasis):\n\n{}", seed),
                }],
            });
            api_messages.push(ApiMessage {
                role: "assistant".to_string(),
                content: vec![ContentBlock::Text { text: "Understood. I will follow these instructions.".to_string() }],
            });
        }
    }

    for (idx, msg) in messages.iter().enumerate() {
        if msg.status == MessageStatus::Deleted || msg.status == MessageStatus::Detached {
            continue;
        }

        if msg.content.is_empty() && msg.tool_uses.is_empty() && msg.tool_results.is_empty() {
            continue;
        }

        let mut content_blocks: Vec<ContentBlock> = Vec::new();

        if msg.message_type == MessageType::ToolResult {
            for result in &msg.tool_results {
                content_blocks.push(ContentBlock::ToolResult {
                    tool_use_id: result.tool_use_id.clone(),
                    content: result.content.clone(),
                });
            }

            if !content_blocks.is_empty() {
                api_messages.push(ApiMessage { role: "user".to_string(), content: content_blocks });
            }
            continue;
        }

        if msg.message_type == MessageType::ToolCall {
            let tool_use_ids: Vec<&str> = msg.tool_uses.iter().map(|t| t.id.as_str()).collect();

            let has_matching_tool_result = messages[idx + 1..]
                .iter()
                .filter(|m| m.status != MessageStatus::Deleted && m.status != MessageStatus::Detached)
                .filter(|m| m.message_type == MessageType::ToolResult)
                .any(|m| m.tool_results.iter().any(|r| tool_use_ids.contains(&r.tool_use_id.as_str())));

            if has_matching_tool_result {
                for tool_use in &msg.tool_uses {
                    let input = if tool_use.input.is_null() {
                        Value::Object(serde_json::Map::new())
                    } else {
                        tool_use.input.clone()
                    };
                    content_blocks.push(ContentBlock::ToolUse {
                        id: tool_use.id.clone(),
                        name: tool_use.name.clone(),
                        input,
                    });
                }

                if let Some(last_api_msg) = api_messages.last_mut()
                    && last_api_msg.role == "assistant"
                {
                    last_api_msg.content.extend(content_blocks);
                    continue;
                }
            } else {
                continue;
            }
        } else {
            let message_content = match msg.status {
                MessageStatus::Summarized => msg.tl_dr.as_ref().unwrap_or(&msg.content).clone(),
                _ => msg.content.clone(),
            };

            if !message_content.is_empty() {
                content_blocks.push(ContentBlock::Text { text: message_content });
            }

            let is_last = idx == messages.len().saturating_sub(1);
            if msg.role == "assistant" && include_last_tool_uses && is_last && !msg.tool_uses.is_empty() {
                for tool_use in &msg.tool_uses {
                    let input = if tool_use.input.is_null() {
                        Value::Object(serde_json::Map::new())
                    } else {
                        tool_use.input.clone()
                    };
                    content_blocks.push(ContentBlock::ToolUse {
                        id: tool_use.id.clone(),
                        name: tool_use.name.clone(),
                        input,
                    });
                }
            }
        }

        if !content_blocks.is_empty() {
            api_messages.push(ApiMessage { role: msg.role.clone(), content: content_blocks });
        }
    }

    api_messages
}
