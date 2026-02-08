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

use super::{ApiCheckResult, LlmClient, LlmRequest, StreamEvent, prepare_panel_messages, panel_header_text, panel_footer_text, panel_timestamp_text};
use crate::constants::{prompts, API_VERSION, MAX_RESPONSE_TOKENS};
use crate::core::panels::now_ms;
use crate::state::{MessageStatus, MessageType};
use crate::tool_defs::build_api_tools;
use crate::tools::ToolUse;

/// API endpoint with beta flag required for Claude 4.5 access
const CLAUDE_CODE_ENDPOINT: &str = "https://api.anthropic.com/v1/messages?beta=true";

/// Beta header with all required flags for Claude Code access
const OAUTH_BETA_HEADER: &str = "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14,context-management-2025-06-27,prompt-caching-scope-2026-01-05";

/// Billing header that must be included in system prompt
const BILLING_HEADER: &str = "x-anthropic-billing-header: cc_version=2.1.37.fbe; cc_entrypoint=cli; cch=e5401;";

/// System reminder injected into first user message for Claude Code validation
const SYSTEM_REMINDER: &str = "<system-reminder>\nThe following skills are available for use with the Skill tool:\n</system-reminder>";

/// Map model names to full API model identifiers
fn map_model_name(model: &str) -> &str {
    match model {
        "claude-opus-4-6" => "claude-opus-4-6",
        "claude-opus-4-5" => "claude-opus-4-6",
        "claude-sonnet-4-5" => "claude-sonnet-4-5-20250929",
        "claude-haiku-4-5" => "claude-haiku-4-5-20251001",
        _ => model,
    }
}

/// Inject the system-reminder text block into the first non-tool-result user message.
/// Claude Code's server validates that messages contain this marker.
/// Must skip tool_result user messages (from panel injection) since mixing text blocks
/// into tool_result messages breaks the API's tool_use/tool_result pairing.
fn inject_system_reminder(messages: &mut Vec<Value>) {
    let reminder = serde_json::json!({"type": "text", "text": SYSTEM_REMINDER});

    for msg in messages.iter_mut() {
        if msg["role"] != "user" {
            continue;
        }

        // Skip tool_result messages (from panel injection / tool loop)
        if let Some(arr) = msg["content"].as_array() {
            if arr.iter().any(|block| block["type"] == "tool_result") {
                continue;
            }
        }

        // Convert string content to array format and prepend reminder
        let content = &msg["content"];
        if content.is_string() {
            let text = content.as_str().unwrap_or("").to_string();
            msg["content"] = serde_json::json!([
                reminder,
                {"type": "text", "text": text}
            ]);
        } else if content.is_array() {
            if let Some(arr) = msg["content"].as_array_mut() {
                arr.insert(0, reminder);
            }
        }
        return; // Only inject into first eligible user message
    }

    // No eligible user message found (all are tool_results, e.g. during tool loop).
    // Prepend a standalone user message with just the reminder at position 0.
    messages.insert(0, serde_json::json!({
        "role": "user",
        "content": [reminder]
    }));
    // Must follow with a minimal assistant ack to maintain user/assistant alternation.
    messages.insert(1, serde_json::json!({
        "role": "assistant",
        "content": [{"type": "text", "text": "ok"}]
    }));
}

/// Ensure strict user/assistant message alternation as required by the API.
/// - Consecutive text-only user messages are merged into one.
/// - Between a tool_result user message and a text user message, a placeholder
///   assistant message is inserted (can't merge these — tool_result + text mixing
///   breaks inject_system_reminder and API validation).
/// - Consecutive assistant messages are merged.
fn ensure_message_alternation(messages: &mut Vec<Value>) {
    if messages.len() <= 1 {
        return;
    }

    let mut result: Vec<Value> = Vec::with_capacity(messages.len());

    for msg in messages.drain(..) {
        let same_role = result.last().map_or(false, |last: &Value| last["role"] == msg["role"]);
        if !same_role {
            let blocks = content_to_blocks(msg["content"].clone());
            result.push(serde_json::json!({"role": msg["role"], "content": blocks}));
            continue;
        }

        let prev_has_tool_result = result.last().map_or(false, |last| {
            last["content"].as_array().map_or(false, |arr| {
                arr.iter().any(|b| b["type"] == "tool_result")
            })
        });
        let curr_has_tool_result = msg["content"].as_array().map_or(false, |arr| {
            arr.iter().any(|b| b["type"] == "tool_result")
        });

        if prev_has_tool_result != curr_has_tool_result {
            // Different content types — insert placeholder assistant to separate them
            result.push(serde_json::json!({
                "role": "assistant",
                "content": [{"type": "text", "text": "ok"}]
            }));
            let blocks = content_to_blocks(msg["content"].clone());
            result.push(serde_json::json!({"role": msg["role"], "content": blocks}));
        } else {
            // Same content type — safe to merge
            let new_blocks = content_to_blocks(msg["content"].clone());
            if let Some(arr) = result.last_mut().unwrap()["content"].as_array_mut() {
                arr.extend(new_blocks);
            }
        }
    }

    // API requires first message to be user role. Panel injection starts with
    // assistant messages, so prepend a placeholder user message if needed.
    if result.first().map_or(false, |m| m["role"] == "assistant") {
        result.insert(0, serde_json::json!({
            "role": "user",
            "content": [{"type": "text", "text": "ok"}]
        }));
    }

    *messages = result;
}

/// Convert content (string or array) to an array of content blocks.
fn content_to_blocks(content: Value) -> Vec<Value> {
    if content.is_string() {
        vec![serde_json::json!({"type": "text", "text": content.as_str().unwrap_or("")})]
    } else if let Some(arr) = content.as_array() {
        arr.clone()
    } else {
        vec![]
    }
}

/// Directory for last-request debug dumps
const LAST_REQUESTS_DIR: &str = ".context-pilot/last_requests";

/// Dump the outgoing API request to disk for debugging.
/// Written to `.context-pilot/last_requests/{worker_id}_last_request.json`.
fn dump_last_request(worker_id: &str, api_request: &Value) {
    let debug = serde_json::json!({
        "request_url": CLAUDE_CODE_ENDPOINT,
        "request_headers": {
            "anthropic-beta": OAUTH_BETA_HEADER,
            "anthropic-version": API_VERSION,
            "user-agent": "claude-cli/2.1.37 (external, cli)",
            "x-app": "cli",
        },
        "request_body": api_request,
    });
    let _ = std::fs::create_dir_all(LAST_REQUESTS_DIR);
    let path = format!("{}/{}_last_request.json", LAST_REQUESTS_DIR, worker_id);
    let _ = std::fs::write(path, serde_json::to_string_pretty(&debug).unwrap_or_default());
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
            prompts::main_system().to_string()
        };

        // Build messages as simple JSON (matching Python example format)
        let mut json_messages: Vec<Value> = Vec::new();
        let current_ms = now_ms();

        // Inject context panels as fake tool call/result pairs (P2+ only, sorted by timestamp)
        let fake_panels = prepare_panel_messages(&request.context_items);

        if !fake_panels.is_empty() {
            for (idx, panel) in fake_panels.iter().enumerate() {
                let timestamp_text = panel_timestamp_text(panel.timestamp_ms, current_ms);
                let text = if idx == 0 {
                    format!("{}\n\n{}", panel_header_text(), timestamp_text)
                } else {
                    timestamp_text
                };

                // Assistant message with tool_use
                json_messages.push(serde_json::json!({
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": text},
                        {
                            "type": "tool_use",
                            "id": format!("panel_{}", panel.panel_id),
                            "name": "dynamic_panel",
                            "input": {"id": panel.panel_id}
                        }
                    ]
                }));

                // User message with tool_result
                json_messages.push(serde_json::json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": format!("panel_{}", panel.panel_id),
                        "content": panel.content
                    }]
                }));
            }

            // Add footer after all panels
            let footer = panel_footer_text(&request.messages, current_ms);
            json_messages.push(serde_json::json!({
                "role": "assistant",
                "content": [
                    {"type": "text", "text": footer},
                    {
                        "type": "tool_use",
                        "id": "panel_footer",
                        "name": "dynamic_panel",
                        "input": {"action": "end_panels"}
                    }
                ]
            }));
            json_messages.push(serde_json::json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "panel_footer",
                    "content": crate::constants::prompts::panel_footer_ack()
                }]
            }));
        }

        // Handle cleaner mode extra context
        if let Some(ref context) = request.extra_context {
            json_messages.push(serde_json::json!({
                "role": "user",
                "content": format!("Please clean up the context to reduce token usage:\n\n{}", context)
            }));
        }

        let include_tool_uses = request.tool_results.is_some();

        // First pass: collect tool_use IDs that have matching results (will be included)
        let mut included_tool_use_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (idx, msg) in request.messages.iter().enumerate() {
            if msg.status == MessageStatus::Deleted || msg.message_type != MessageType::ToolCall {
                continue;
            }
            let tool_use_ids: Vec<&str> = msg.tool_uses.iter().map(|t| t.id.as_str()).collect();
            let has_result = request.messages[idx + 1..]
                .iter()
                .filter(|m| m.status != MessageStatus::Deleted && m.message_type == MessageType::ToolResult)
                .any(|m| m.tool_results.iter().any(|r| tool_use_ids.contains(&r.tool_use_id.as_str())));
            if has_result {
                for id in tool_use_ids {
                    included_tool_use_ids.insert(id.to_string());
                }
            }
        }

        for (idx, msg) in request.messages.iter().enumerate() {
            if msg.status == MessageStatus::Deleted {
                continue;
            }
            if msg.content.is_empty() && msg.tool_uses.is_empty() && msg.tool_results.is_empty() {
                continue;
            }

            // Handle tool results - only include if tool_use was included
            if msg.message_type == MessageType::ToolResult {
                let tool_results: Vec<Value> = msg.tool_results.iter()
                    .filter(|r| included_tool_use_ids.contains(&r.tool_use_id))
                    .map(|r| {
                        serde_json::json!({
                            "type": "tool_result",
                            "tool_use_id": r.tool_use_id,
                            "content": format!("[{}]:\n{}", msg.id, r.content)
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

            // Handle tool calls - only include if has matching result
            if msg.message_type == MessageType::ToolCall {
                let tool_uses: Vec<Value> = msg.tool_uses.iter()
                    .filter(|tu| included_tool_use_ids.contains(&tu.id))
                    .map(|tu| {
                        serde_json::json!({
                            "type": "tool_use",
                            "id": tu.id,
                            "name": tu.name,
                            "input": if tu.input.is_null() { serde_json::json!({}) } else { tu.input.clone() }
                        })
                    }).collect();

                if !tool_uses.is_empty() {
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
                // Use [ID]:\n format (newline after colon)
                let prefixed = format!("[{}]:\n{}", msg.id, message_content);

                // Use simple string content like Python example
                json_messages.push(serde_json::json!({
                    "role": msg.role,
                    "content": prefixed
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

        // Ensure strict user/assistant alternation (merges consecutive same-role messages)
        ensure_message_alternation(&mut json_messages);

        // Inject system-reminder into first user message for Claude Code validation
        inject_system_reminder(&mut json_messages);

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

        // Always dump last request for debugging (overwritten each call)
        dump_last_request(&request.worker_id, &api_request);

        let response = client
            .post(CLAUDE_CODE_ENDPOINT)
            .header("accept", "text/event-stream")
            .header("authorization", format!("Bearer {}", access_token))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", OAUTH_BETA_HEADER)
            .header("anthropic-dangerous-direct-browser-access", "true")
            .header("content-type", "application/json")
            .header("user-agent", "claude-cli/2.1.37 (external, cli)")
            .header("x-app", "cli")
            .header("x-stainless-arch", "x64")
            .header("x-stainless-lang", "js")
            .header("x-stainless-os", "Linux")
            .header("x-stainless-package-version", "0.70.0")
            .header("x-stainless-retry-count", "0")
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

        // User message with system-reminder injected (required by server validation)
        let user_msg = serde_json::json!({
            "role": "user",
            "content": [
                {"type": "text", "text": SYSTEM_REMINDER},
                {"type": "text", "text": "Hi"}
            ]
        });

        // Test 1: Basic auth with simple non-streaming request
        let auth_result = client
            .post(CLAUDE_CODE_ENDPOINT)
            .header("accept", "application/json")
            .header("authorization", format!("Bearer {}", access_token))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", OAUTH_BETA_HEADER)
            .header("anthropic-dangerous-direct-browser-access", "true")
            .header("content-type", "application/json")
            .header("user-agent", "claude-cli/2.1.37 (external, cli)")
            .header("x-app", "cli")
            .header("x-stainless-arch", "x64")
            .header("x-stainless-lang", "js")
            .header("x-stainless-os", "Linux")
            .header("x-stainless-package-version", "0.70.0")
            .header("x-stainless-retry-count", "0")
            .header("x-stainless-runtime", "node")
            .header("x-stainless-runtime-version", "v24.3.0")
            .json(&serde_json::json!({
                "model": mapped_model,
                "max_tokens": 10,
                "system": system,
                "messages": [user_msg]
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
        let stream_msg = serde_json::json!({
            "role": "user",
            "content": [
                {"type": "text", "text": SYSTEM_REMINDER},
                {"type": "text", "text": "Say ok"}
            ]
        });
        let stream_result = client
            .post(CLAUDE_CODE_ENDPOINT)
            .header("accept", "text/event-stream")
            .header("authorization", format!("Bearer {}", access_token))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", OAUTH_BETA_HEADER)
            .header("anthropic-dangerous-direct-browser-access", "true")
            .header("content-type", "application/json")
            .header("user-agent", "claude-cli/2.1.37 (external, cli)")
            .header("x-app", "cli")
            .header("x-stainless-arch", "x64")
            .header("x-stainless-lang", "js")
            .header("x-stainless-os", "Linux")
            .header("x-stainless-package-version", "0.70.0")
            .header("x-stainless-retry-count", "0")
            .header("x-stainless-runtime", "node")
            .header("x-stainless-runtime-version", "v24.3.0")
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
        let tools_result = client
            .post(CLAUDE_CODE_ENDPOINT)
            .header("accept", "application/json")
            .header("authorization", format!("Bearer {}", access_token))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", OAUTH_BETA_HEADER)
            .header("anthropic-dangerous-direct-browser-access", "true")
            .header("content-type", "application/json")
            .header("user-agent", "claude-cli/2.1.37 (external, cli)")
            .header("x-app", "cli")
            .header("x-stainless-arch", "x64")
            .header("x-stainless-lang", "js")
            .header("x-stainless-os", "Linux")
            .header("x-stainless-package-version", "0.70.0")
            .header("x-stainless-retry-count", "0")
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
                "messages": [tools_msg]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::API_VERSION;

    /// Minimal request matching working Python exactly.
    /// No panels, no tools, no message prefixes — just raw API call.
    #[test]
    fn test_general_kenobi() {
        let token = ClaudeCodeClient::load_oauth_token()
            .expect("OAuth token not found or expired — run 'claude login'");

        let client = Client::new();

        // Exact same payload structure as working Python create_payload()
        let body = serde_json::json!({
            "model": "claude-opus-4-6",
            "max_tokens": 100,
            "system": [
                {"type": "text", "text": BILLING_HEADER},
                {"type": "text", "text": "You are a helpful assistant."}
            ],
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": SYSTEM_REMINDER},
                    {"type": "text", "text": "Hello There! (this is a test, answer General Kenobi)"}
                ]
            }]
        });

        // Exact same headers as working Python get_claude_code_headers()
        let response = client
            .post(CLAUDE_CODE_ENDPOINT)
            .header("accept", "application/json")
            .header("authorization", format!("Bearer {}", token))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", OAUTH_BETA_HEADER)
            .header("anthropic-dangerous-direct-browser-access", "true")
            .header("content-type", "application/json")
            .header("user-agent", "claude-cli/2.1.37 (external, cli)")
            .header("x-app", "cli")
            .header("x-stainless-arch", "x64")
            .header("x-stainless-lang", "js")
            .header("x-stainless-os", "Linux")
            .header("x-stainless-package-version", "0.70.0")
            .header("x-stainless-retry-count", "0")
            .header("x-stainless-runtime", "node")
            .header("x-stainless-runtime-version", "v24.3.0")
            .json(&body)
            .send()
            .expect("HTTP request failed");

        let status = response.status();
        let resp_body: serde_json::Value = response.json().expect("Failed to parse JSON response");

        assert!(
            status.is_success(),
            "API returned {}: {}",
            status,
            serde_json::to_string_pretty(&resp_body).unwrap()
        );

        let text = resp_body["content"][0]["text"]
            .as_str()
            .expect("No text in response content");

        assert!(
            text.to_lowercase().contains("general kenobi"),
            "Expected 'General Kenobi' in response, got: {}",
            text
        );
    }

    /// Same as above but with tools and streaming — matches what stream() actually sends.
    #[test]
    fn test_general_kenobi_with_tools_streaming() {
        let token = ClaudeCodeClient::load_oauth_token()
            .expect("OAuth token not found or expired — run 'claude login'");

        let client = Client::new();

        // Mimic the stream() method: tools array, streaming, max_tokens=4096
        let body = serde_json::json!({
            "model": "claude-opus-4-6",
            "max_tokens": 4096,
            "system": [
                {"type": "text", "text": BILLING_HEADER},
                {"type": "text", "text": "You are a helpful assistant."}
            ],
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": SYSTEM_REMINDER},
                    {"type": "text", "text": "Hello There! (this is a test, answer General Kenobi)"}
                ]
            }],
            "tools": [{
                "name": "test_tool",
                "description": "A test tool",
                "input_schema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            }],
            "stream": true
        });

        let response = client
            .post(CLAUDE_CODE_ENDPOINT)
            .header("accept", "text/event-stream")
            .header("authorization", format!("Bearer {}", token))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", OAUTH_BETA_HEADER)
            .header("anthropic-dangerous-direct-browser-access", "true")
            .header("content-type", "application/json")
            .header("user-agent", "claude-cli/2.1.37 (external, cli)")
            .header("x-app", "cli")
            .header("x-stainless-arch", "x64")
            .header("x-stainless-lang", "js")
            .header("x-stainless-os", "Linux")
            .header("x-stainless-package-version", "0.70.0")
            .header("x-stainless-retry-count", "0")
            .header("x-stainless-runtime", "node")
            .header("x-stainless-runtime-version", "v24.3.0")
            .json(&body)
            .send()
            .expect("HTTP request failed");

        let status = response.status();

        // For streaming, read SSE lines and collect text
        assert!(status.is_success(), "API returned {}", status);

        let mut full_text = String::new();
        let reader = std::io::BufReader::new(response);
        for line in reader.lines() {
            let line = line.expect("Read error");
            if !line.starts_with("data: ") { continue; }
            let json_str = &line[6..];
            if json_str == "[DONE]" { break; }
            if let Ok(event) = serde_json::from_str::<Value>(json_str) {
                if event["type"] == "content_block_delta" {
                    if let Some(text) = event["delta"]["text"].as_str() {
                        full_text.push_str(text);
                    }
                }
            }
        }

        assert!(
            full_text.to_lowercase().contains("general kenobi"),
            "Expected 'General Kenobi' in streamed response, got: {}",
            full_text
        );
    }

    /// Test inject_system_reminder: verify it skips tool_result messages
    /// and injects into the first regular user message.
    #[test]
    fn test_inject_system_reminder_skips_tool_results() {
        // Simulate panel injection: tool_result user messages first, then a regular user msg
        let mut messages = vec![
            serde_json::json!({
                "role": "assistant",
                "content": [{"type": "text", "text": "panel"}, {"type": "tool_use", "id": "panel_P2", "name": "dynamic_panel", "input": {}}]
            }),
            serde_json::json!({
                "role": "user",
                "content": [{"type": "tool_result", "tool_use_id": "panel_P2", "content": "data"}]
            }),
            serde_json::json!({
                "role": "user",
                "content": "Hello there"
            }),
        ];

        inject_system_reminder(&mut messages);

        // tool_result message should be untouched
        assert!(
            messages[1]["content"][0]["type"] == "tool_result",
            "tool_result message was modified: {:?}",
            messages[1]["content"]
        );

        // Regular user message should now be an array with reminder first
        assert!(
            messages[2]["content"].is_array(),
            "Regular user message not converted to array"
        );
        let arr = messages[2]["content"].as_array().unwrap();
        assert_eq!(arr.len(), 2, "Expected 2 blocks (reminder + text)");
        assert!(
            arr[0]["text"].as_str().unwrap().contains("system-reminder"),
            "First block should be system-reminder"
        );
        assert_eq!(
            arr[1]["text"].as_str().unwrap(),
            "Hello there"
        );
    }

    /// Test inject_system_reminder: when no eligible message, prepends fallback pair
    #[test]
    fn test_inject_system_reminder_no_eligible() {
        // Only tool_result user messages — triggers fallback
        let mut messages = vec![
            serde_json::json!({
                "role": "assistant",
                "content": [{"type": "tool_use", "id": "t1", "name": "x", "input": {}}]
            }),
            serde_json::json!({
                "role": "user",
                "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "data"}]
            }),
        ];

        inject_system_reminder(&mut messages);

        // Should have 4 messages: [fallback_user, fallback_assistant, original_assistant, original_user]
        assert_eq!(messages.len(), 4);
        // First message is the fallback user with reminder
        assert_eq!(messages[0]["role"], "user");
        assert!(messages[0]["content"][0]["text"].as_str().unwrap().contains("system-reminder"));
        // Second message is the assistant ack
        assert_eq!(messages[1]["role"], "assistant");
        // Original messages preserved at indices 2 and 3
        assert!(messages[3]["content"][0]["type"] == "tool_result");
    }

    /// Test ensure_message_alternation: merges consecutive text user messages,
    /// but separates tool_result user from text user with a placeholder assistant.
    #[test]
    fn test_ensure_message_alternation() {
        // Simulate the actual failure scenario: panel footer (tool_result user)
        // followed by consecutive text user messages
        let mut messages = vec![
            serde_json::json!({"role": "assistant", "content": [{"type": "text", "text": "panel"}]}),
            serde_json::json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "panel_footer", "content": "ok"}]}),
            // These 3 consecutive text user messages should be merged, with a
            // placeholder assistant separating them from the tool_result above
            serde_json::json!({"role": "user", "content": "Hello"}),
            serde_json::json!({"role": "user", "content": "World"}),
            serde_json::json!({"role": "user", "content": "Again"}),
        ];

        ensure_message_alternation(&mut messages);

        // Should have 5: prepended user "ok", assistant, tool_result user, placeholder assistant, merged text user
        assert_eq!(messages.len(), 5, "Got {} messages: {:?}",
            messages.len(), messages.iter().map(|m| m["role"].as_str().unwrap_or("?")).collect::<Vec<_>>());
        assert_eq!(messages[0]["role"], "user"); // prepended because first msg was assistant
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"][0]["type"], "tool_result");
        assert_eq!(messages[3]["role"], "assistant"); // placeholder
        assert_eq!(messages[4]["role"], "user");

        // The merged text user message has all 3 texts
        let user_content = messages[4]["content"].as_array().unwrap();
        assert_eq!(user_content.len(), 3);
        assert_eq!(user_content[0]["text"], "Hello");
        assert_eq!(user_content[1]["text"], "World");
        assert_eq!(user_content[2]["text"], "Again");
    }

    /// Test that alternation + reminder injection work together on the real scenario
    #[test]
    fn test_alternation_then_reminder() {
        let mut messages = vec![
            serde_json::json!({"role": "assistant", "content": [{"type": "text", "text": "footer"}, {"type": "tool_use", "id": "panel_footer", "name": "dynamic_panel", "input": {}}]}),
            serde_json::json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "panel_footer", "content": "ok"}]}),
            serde_json::json!({"role": "user", "content": "Hello"}),
            serde_json::json!({"role": "user", "content": "World"}),
        ];

        ensure_message_alternation(&mut messages);
        inject_system_reminder(&mut messages);

        // 5 messages: prepended user (gets reminder), assistant, tool_result user, placeholder assistant, merged text user
        assert_eq!(messages.len(), 5);
        // The prepended user message (index 0) should have system-reminder injected
        let user_content = messages[0]["content"].as_array().unwrap();
        assert!(user_content[0]["text"].as_str().unwrap().contains("system-reminder"),
            "First block should be system-reminder, got: {:?}", user_content[0]);
    }
}

