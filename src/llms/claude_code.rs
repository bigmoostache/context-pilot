//! Claude Code OAuth API implementation.
//!
//! Uses OAuth tokens from ~/.claude/.credentials.json with Bearer authentication.

use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ApiCheckResult, ApiMessage, ContentBlock, LlmClient, LlmRequest, StreamEvent};
use crate::constants::{prompts, API_ENDPOINT, API_VERSION, MAX_RESPONSE_TOKENS};
use crate::panels::ContextItem;
use crate::state::{Message, MessageStatus, MessageType};
use crate::tool_defs::build_api_tools;
use crate::tools::ToolUse;

const OAUTH_BETA_HEADER: &str = "oauth-2025-04-20";

/// Map Claude 4.5 models to 3.5 equivalents (OAuth doesn't support 4.x models)
fn map_model_for_oauth(model: &str) -> &str {
    match model {
        "claude-opus-4-5" | "claude-opus-4-5-latest" => "claude-3-5-sonnet-20241022", // No 3.5 opus, use sonnet
        "claude-sonnet-4-5" | "claude-sonnet-4-5-latest" => "claude-3-5-sonnet-20241022",
        "claude-haiku-4-5" | "claude-haiku-4-5-latest" => "claude-3-5-haiku-20241022",
        _ => model, // Pass through unknown models
    }
}

/// Claude Code OAuth client
pub struct ClaudeCodeClient {
    access_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: OAuthCredentials,
}

#[derive(Debug, Deserialize)]
struct OAuthCredentials {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "expiresAt")]
    expires_at: u64,
}

impl ClaudeCodeClient {
    pub fn new() -> Self {
        let access_token = Self::load_oauth_token();
        Self { access_token }
    }

    fn load_oauth_token() -> Option<String> {
        let home = env::var("HOME").ok()?;
        let home_path = PathBuf::from(&home);

        // Try hidden credentials file first
        let creds_path = home_path.join(".claude").join(".credentials.json");
        let path = if creds_path.exists() {
            creds_path
        } else {
            // Fallback to non-hidden
            home_path.join(".claude").join("credentials.json")
        };

        let content = fs::read_to_string(&path).ok()?;
        let creds: CredentialsFile = serde_json::from_str(&content).ok()?;

        // Check if token is expired
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis() as u64;

        if now_ms > creds.claude_ai_oauth.expires_at {
            return None; // Token expired
        }

        Some(creds.claude_ai_oauth.access_token)
    }
}

impl Default for ClaudeCodeClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Serialize)]
struct ClaudeCodeRequest {
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
}

#[derive(Debug, Deserialize)]
struct StreamMessage {
    #[serde(rename = "type")]
    event_type: String,
    content_block: Option<StreamContentBlock>,
    delta: Option<StreamDelta>,
    usage: Option<StreamUsage>,
}

#[derive(Debug, Deserialize)]
struct StreamUsage {
    input_tokens: Option<usize>,
    output_tokens: Option<usize>,
}

impl LlmClient for ClaudeCodeClient {
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), String> {
        let access_token = self
            .access_token
            .clone()
            .ok_or_else(|| "Claude Code OAuth token not found or expired. Run 'claude login'".to_string())?;

        let client = Client::new();

        // Build API messages
        let include_tool_uses = request.tool_results.is_some();
        let mut api_messages =
            messages_to_api(&request.messages, &request.context_items, include_tool_uses);

        // Add tool results if present
        if let Some(results) = &request.tool_results {
            let tool_result_blocks: Vec<ContentBlock> = results
                .iter()
                .map(|r| ContentBlock::ToolResult {
                    tool_use_id: r.tool_use_id.clone(),
                    content: r.content.clone(),
                })
                .collect();

            api_messages.push(ApiMessage {
                role: "user".to_string(),
                content: tool_result_blocks,
            });
        }

        // Handle cleaner mode or custom system prompt
        let system_prompt = if let Some(ref prompt) = request.system_prompt {
            if let Some(ref context) = request.extra_context {
                api_messages.push(ApiMessage {
                    role: "user".to_string(),
                    content: vec![ContentBlock::Text {
                        text: format!(
                            "Please clean up the context to reduce token usage:\n\n{}",
                            context
                        ),
                    }],
                });
            }
            prompt.clone()
        } else {
            prompts::MAIN_SYSTEM.to_string()
        };

        let api_request = ClaudeCodeRequest {
            model: map_model_for_oauth(&request.model).to_string(),
            max_tokens: MAX_RESPONSE_TOKENS,
            system: system_prompt,
            messages: api_messages,
            tools: build_api_tools(&request.tools),
            stream: true,
        };

        let response = client
            .post(API_ENDPOINT)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", OAUTH_BETA_HEADER)
            .header("content-type", "application/json")
            .json(&api_request)
            .send()
            .map_err(|e| format!("Request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(format!("API error {}: {}", status, body));
        }

        let reader = BufReader::new(response);
        let mut input_tokens = 0;
        let mut output_tokens = 0;
        let mut current_tool: Option<(String, String, String)> = None;

        for line in reader.lines() {
            let line = line.map_err(|e| format!("Read error: {}", e))?;

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
                        if let Some(block) = event.content_block {
                            if block.block_type.as_deref() == Some("tool_use") {
                                current_tool = Some((
                                    block.id.unwrap_or_default(),
                                    block.name.unwrap_or_default(),
                                    String::new(),
                                ));
                            }
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
                                    if let Some(json) = delta.partial_json {
                                        if let Some((_, _, ref mut input)) = current_tool {
                                            input.push_str(&json);
                                        }
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
        });
        Ok(())
    }

    fn check_api(&self, model: &str) -> ApiCheckResult {
        let access_token = match &self.access_token {
            Some(t) => t.clone(),
            None => {
                return ApiCheckResult {
                    auth_ok: false,
                    streaming_ok: false,
                    tools_ok: false,
                    error: Some("OAuth token not found or expired".to_string()),
                }
            }
        };

        let client = Client::new();
        let mapped_model = map_model_for_oauth(model);

        // Test 1: Basic auth with simple non-streaming request
        let auth_result = client
            .post(API_ENDPOINT)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", OAUTH_BETA_HEADER)
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "model": mapped_model,
                "max_tokens": 10,
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .send();

        let auth_ok = match &auth_result {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        };

        if !auth_ok {
            let error = auth_result
                .err()
                .map(|e| e.to_string())
                .or_else(|| Some("Auth failed".to_string()));
            return ApiCheckResult {
                auth_ok: false,
                streaming_ok: false,
                tools_ok: false,
                error,
            };
        }

        // Test 2: Streaming request
        let stream_result = client
            .post(API_ENDPOINT)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", OAUTH_BETA_HEADER)
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "model": mapped_model,
                "max_tokens": 10,
                "stream": true,
                "messages": [{"role": "user", "content": "Say ok"}]
            }))
            .send();

        let streaming_ok = stream_result.as_ref().map(|r| r.status().is_success()).unwrap_or(false);

        // Test 3: Tool calling
        let tools_result = client
            .post(API_ENDPOINT)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", OAUTH_BETA_HEADER)
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "model": mapped_model,
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

        ApiCheckResult {
            auth_ok,
            streaming_ok,
            tools_ok,
            error: None,
        }
    }
}

/// Convert internal messages to API format (same as anthropic.rs)
fn messages_to_api(
    messages: &[Message],
    context_items: &[ContextItem],
    include_last_tool_uses: bool,
) -> Vec<ApiMessage> {
    let mut api_messages: Vec<ApiMessage> = Vec::new();

    let context_parts: Vec<String> = context_items
        .iter()
        .filter(|item| !item.content.is_empty())
        .map(|item| item.format())
        .collect();

    for (idx, msg) in messages.iter().enumerate() {
        if msg.status == MessageStatus::Deleted {
            continue;
        }

        if msg.content.is_empty() && msg.tool_uses.is_empty() && msg.tool_results.is_empty() {
            continue;
        }

        let mut content_blocks: Vec<ContentBlock> = Vec::new();

        if msg.message_type == MessageType::ToolResult {
            for result in &msg.tool_results {
                let prefixed_content = format!("[{}]: {}", msg.id, result.content);
                content_blocks.push(ContentBlock::ToolResult {
                    tool_use_id: result.tool_use_id.clone(),
                    content: prefixed_content,
                });
            }

            if !content_blocks.is_empty() {
                api_messages.push(ApiMessage {
                    role: "user".to_string(),
                    content: content_blocks,
                });
            }
            continue;
        }

        if msg.message_type == MessageType::ToolCall {
            let tool_use_ids: Vec<&str> = msg.tool_uses.iter().map(|t| t.id.as_str()).collect();

            let has_matching_tool_result = messages[idx + 1..]
                .iter()
                .filter(|m| m.status != MessageStatus::Deleted)
                .filter(|m| m.message_type == MessageType::ToolResult)
                .any(|m| {
                    m.tool_results
                        .iter()
                        .any(|r| tool_use_ids.contains(&r.tool_use_id.as_str()))
                });

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

                if let Some(last_api_msg) = api_messages.last_mut() {
                    if last_api_msg.role == "assistant" {
                        last_api_msg.content.extend(content_blocks);
                        continue;
                    }
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
                let prefixed_content = format!("[{}]: {}", msg.id, message_content);

                let text =
                    if msg.role == "user" && !context_parts.is_empty() && api_messages.is_empty() {
                        let context = context_parts.join("\n\n");
                        format!("{}\n\n{}", context, prefixed_content)
                    } else {
                        prefixed_content
                    };
                content_blocks.push(ContentBlock::Text { text });
            }

            let is_last = idx == messages.len().saturating_sub(1);
            if msg.role == "assistant"
                && include_last_tool_uses
                && is_last
                && !msg.tool_uses.is_empty()
            {
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
            api_messages.push(ApiMessage {
                role: msg.role.clone(),
                content: content_blocks,
            });
        }
    }

    api_messages
}
