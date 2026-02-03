//! Claude Code OAuth API implementation.
//!
//! Uses OAuth tokens from ~/.claude/.credentials.json with Bearer authentication.
//! Replicates Claude Code's request signature to access Claude 4.5 models.

use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::Value;

use super::{ApiCheckResult, LlmClient, LlmRequest, StreamEvent};
use crate::constants::{prompts, API_VERSION, MAX_RESPONSE_TOKENS};
use crate::state::{MessageStatus, MessageType};
use crate::tool_defs::build_api_tools;
use crate::tools::ToolUse;

/// API endpoint with beta flag required for Claude 4.5 access
const CLAUDE_CODE_ENDPOINT: &str = "https://api.anthropic.com/v1/messages?beta=true";

/// Beta header with all required flags for Claude Code access
const OAUTH_BETA_HEADER: &str = "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14";

/// Billing header that must be included in system prompt
const BILLING_HEADER: &str = "x-anthropic-billing-header: cc_version=2.1.29.87a; cc_entrypoint=cli;";

/// Map model names to full API model identifiers
fn map_model_name(model: &str) -> &str {
    match model {
        "claude-opus-4-5" => "claude-opus-4-5-20251101",
        "claude-sonnet-4-5" => "claude-sonnet-4-5-20250514",
        "claude-haiku-4-5" => "claude-haiku-4-5-20250514",
        _ => model,
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

        // Handle cleaner mode or custom system prompt
        let system_text = if let Some(ref prompt) = request.system_prompt {
            prompt.clone()
        } else {
            prompts::MAIN_SYSTEM.to_string()
        };

        // Build messages as simple JSON (matching Python example format)
        let mut json_messages: Vec<Value> = Vec::new();

        // Add context to first user message
        let context_parts: Vec<String> = request.context_items
            .iter()
            .filter(|item| !item.content.is_empty())
            .map(|item| item.format())
            .collect();

        // Handle cleaner mode extra context
        if let Some(ref context) = request.extra_context {
            json_messages.push(serde_json::json!({
                "role": "user",
                "content": format!("Please clean up the context to reduce token usage:\n\n{}", context)
            }));
        }

        let mut first_user_message = true;
        let include_tool_uses = request.tool_results.is_some();

        for (idx, msg) in request.messages.iter().enumerate() {
            if msg.status == MessageStatus::Deleted {
                continue;
            }
            if msg.content.is_empty() && msg.tool_uses.is_empty() && msg.tool_results.is_empty() {
                continue;
            }

            // Handle tool results
            if msg.message_type == MessageType::ToolResult {
                let tool_results: Vec<Value> = msg.tool_results.iter().map(|r| {
                    serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": r.tool_use_id,
                        "content": format!("[{}]: {}", msg.id, r.content)
                    })
                }).collect();

                if !tool_results.is_empty() {
                    json_messages.push(serde_json::json!({
                        "role": "user",
                        "content": tool_results
                    }));
                }
                continue;
            }

            // Handle tool calls
            if msg.message_type == MessageType::ToolCall {
                let tool_use_ids: Vec<&str> = msg.tool_uses.iter().map(|t| t.id.as_str()).collect();
                let has_result = request.messages[idx + 1..]
                    .iter()
                    .filter(|m| m.status != MessageStatus::Deleted && m.message_type == MessageType::ToolResult)
                    .any(|m| m.tool_results.iter().any(|r| tool_use_ids.contains(&r.tool_use_id.as_str())));

                if has_result {
                    let tool_uses: Vec<Value> = msg.tool_uses.iter().map(|tu| {
                        serde_json::json!({
                            "type": "tool_use",
                            "id": tu.id,
                            "name": tu.name,
                            "input": if tu.input.is_null() { serde_json::json!({}) } else { tu.input.clone() }
                        })
                    }).collect();

                    // Append to last assistant message or create new one
                    if let Some(last) = json_messages.last_mut() {
                        if last["role"] == "assistant" {
                            if let Some(content) = last.get_mut("content") {
                                if let Some(arr) = content.as_array_mut() {
                                    arr.extend(tool_uses);
                                    continue;
                                }
                            }
                        }
                    }

                    json_messages.push(serde_json::json!({
                        "role": "assistant",
                        "content": tool_uses
                    }));
                }
                continue;
            }

            // Regular text message
            let message_content = match msg.status {
                MessageStatus::Summarized => msg.tl_dr.as_ref().unwrap_or(&msg.content).clone(),
                _ => msg.content.clone(),
            };

            if !message_content.is_empty() {
                let prefixed = format!("[{}]: {}", msg.id, message_content);
                let text = if msg.role == "user" && first_user_message && !context_parts.is_empty() {
                    first_user_message = false;
                    format!("{}\n\n{}", context_parts.join("\n\n"), prefixed)
                } else {
                    if msg.role == "user" {
                        first_user_message = false;
                    }
                    prefixed
                };

                // Use simple string content like Python example
                json_messages.push(serde_json::json!({
                    "role": msg.role,
                    "content": text
                }));
            }

            // Add tool uses to last assistant message if this is the last message
            let is_last = idx == request.messages.len().saturating_sub(1);
            if msg.role == "assistant" && include_tool_uses && is_last && !msg.tool_uses.is_empty() {
                let tool_uses: Vec<Value> = msg.tool_uses.iter().map(|tu| {
                    serde_json::json!({
                        "type": "tool_use",
                        "id": tu.id,
                        "name": tu.name,
                        "input": if tu.input.is_null() { serde_json::json!({}) } else { tu.input.clone() }
                    })
                }).collect();

                if let Some(last) = json_messages.last_mut() {
                    if last["role"] == "assistant" {
                        // Convert string content to array and add tool uses
                        let existing_content = last["content"].clone();
                        let mut content_array = if existing_content.is_string() {
                            vec![serde_json::json!({"type": "text", "text": existing_content.as_str().unwrap_or("")})]
                        } else if let Some(arr) = existing_content.as_array() {
                            arr.clone()
                        } else {
                            vec![]
                        };
                        content_array.extend(tool_uses);
                        last["content"] = Value::Array(content_array);
                    }
                }
            }
        }

        // Add pending tool results
        if let Some(results) = &request.tool_results {
            let tool_results: Vec<Value> = results.iter().map(|r| {
                serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": r.tool_use_id,
                    "content": r.content
                })
            }).collect();
            json_messages.push(serde_json::json!({
                "role": "user",
                "content": tool_results
            }));
        }

        // Build final request using json!() macro (matching Python exactly)
        let api_request = serde_json::json!({
            "model": map_model_name(&request.model),
            "max_tokens": MAX_RESPONSE_TOKENS,
            "system": [
                {"type": "text", "text": BILLING_HEADER},
                {"type": "text", "text": system_text}
            ],
            "messages": json_messages,
            "tools": build_api_tools(&request.tools),
            "stream": true
        });

        let response = client
            .post(CLAUDE_CODE_ENDPOINT)
            .header("accept", "text/event-stream")
            .header("authorization", format!("Bearer {}", access_token))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", OAUTH_BETA_HEADER)
            .header("anthropic-dangerous-direct-browser-access", "true")
            .header("content-type", "application/json")
            .header("user-agent", "claude-cli/2.1.29 (external, cli)")
            .header("x-app", "cli")
            .header("x-stainless-lang", "js")
            .header("x-stainless-os", "Linux")
            .header("x-stainless-package-version", "0.70.0")
            .header("x-stainless-runtime", "node")
            .header("x-stainless-runtime-version", "v24.3.0")
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
        let mapped_model = map_model_name(model);

        // System with billing header
        let system = serde_json::json!([
            {"type": "text", "text": BILLING_HEADER},
            {"type": "text", "text": "You are a helpful assistant."}
        ]);

        // Test 1: Basic auth with simple non-streaming request
        let auth_result = client
            .post(CLAUDE_CODE_ENDPOINT)
            .header("accept", "application/json")
            .header("authorization", format!("Bearer {}", access_token))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", OAUTH_BETA_HEADER)
            .header("anthropic-dangerous-direct-browser-access", "true")
            .header("content-type", "application/json")
            .header("user-agent", "claude-cli/2.1.29 (external, cli)")
            .header("x-app", "cli")
            .header("x-stainless-lang", "js")
            .header("x-stainless-os", "Linux")
            .header("x-stainless-package-version", "0.70.0")
            .header("x-stainless-runtime", "node")
            .header("x-stainless-runtime-version", "v24.3.0")
            .json(&serde_json::json!({
                "model": mapped_model,
                "max_tokens": 10,
                "system": system,
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
            .post(CLAUDE_CODE_ENDPOINT)
            .header("accept", "text/event-stream")
            .header("authorization", format!("Bearer {}", access_token))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", OAUTH_BETA_HEADER)
            .header("anthropic-dangerous-direct-browser-access", "true")
            .header("content-type", "application/json")
            .header("user-agent", "claude-cli/2.1.29 (external, cli)")
            .header("x-app", "cli")
            .header("x-stainless-lang", "js")
            .header("x-stainless-os", "Linux")
            .header("x-stainless-package-version", "0.70.0")
            .header("x-stainless-runtime", "node")
            .header("x-stainless-runtime-version", "v24.3.0")
            .json(&serde_json::json!({
                "model": mapped_model,
                "max_tokens": 10,
                "stream": true,
                "system": system,
                "messages": [{"role": "user", "content": "Say ok"}]
            }))
            .send();

        let streaming_ok = stream_result.as_ref().map(|r| r.status().is_success()).unwrap_or(false);

        // Test 3: Tool calling
        let tools_result = client
            .post(CLAUDE_CODE_ENDPOINT)
            .header("accept", "application/json")
            .header("authorization", format!("Bearer {}", access_token))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", OAUTH_BETA_HEADER)
            .header("anthropic-dangerous-direct-browser-access", "true")
            .header("content-type", "application/json")
            .header("user-agent", "claude-cli/2.1.29 (external, cli)")
            .header("x-app", "cli")
            .header("x-stainless-lang", "js")
            .header("x-stainless-os", "Linux")
            .header("x-stainless-package-version", "0.70.0")
            .header("x-stainless-runtime", "node")
            .header("x-stainless-runtime-version", "v24.3.0")
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

