//! Claude Code OAuth API implementation.
//!
//! Uses OAuth tokens from ~/.claude/.credentials.json with Bearer authentication.
//! Replicates Claude Code's request signature to access Claude 4.5 models.

use std::env;
use std::fs;
use std::io::{BufRead as _, BufReader};
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use reqwest::blocking::Client;
use secrecy::{ExposeSecret as _, SecretBox};
use serde::Deserialize;
use serde_json::Value;

use super::error::LlmError;
use super::{ApiCheckResult, LlmClient, LlmRequest, StreamEvent};
use crate::infra::constants::{API_VERSION, library};
use crate::infra::tools::{ToolUse, build_api};
use cp_base::cast::SafeCast as _;
use cp_base::config::INJECTIONS;

/// API endpoint with beta flag required for Claude 4.5 access
const CLAUDE_CODE_ENDPOINT: &str = "https://api.anthropic.com/v1/messages?beta=true";

/// Beta header with all required flags for Claude Code access
const OAUTH_BETA_HEADER: &str = "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14,context-management-2025-06-27,prompt-caching-scope-2026-01-05";

/// Billing header that must be included in system prompt
const BILLING_HEADER: &str = "x-anthropic-billing-header: cc_version=2.1.37.fbe; cc_entrypoint=cli; cch=e5401;";

/// System reminder injected into first user message for Claude Code validation
const SYSTEM_REMINDER: &str =
    "<system-reminder>\nThe following skills are available for use with the Skill tool:\n</system-reminder>";

/// Map model names to full API model identifiers
fn map_model_name(model: &str) -> &str {
    match model {
        "claude-opus-4-6" | "claude-opus-4-5" => "claude-opus-4-6",
        "claude-sonnet-4-5" => "claude-sonnet-4-5-20250929",
        "claude-haiku-4-5" => "claude-haiku-4-5-20251001",
        _ => model,
    }
}

/// Inject the system-reminder text block into the first non-tool-result user message.
/// Claude Code's server validates that messages contain this marker.
/// Must skip `tool_result` user messages (from panel injection) since mixing text blocks
/// into `tool_result` messages breaks the API's `tool_use/tool_result` pairing.
fn inject_system_reminder(messages: &mut Vec<Value>) {
    let reminder = serde_json::json!({"type": "text", "text": SYSTEM_REMINDER});

    for msg in messages.iter_mut() {
        if msg["role"] != "user" {
            continue;
        }

        // Skip tool_result messages (from panel injection / tool loop)
        if let Some(arr) = msg["content"].as_array()
            && arr.iter().any(|block| block["type"] == "tool_result")
        {
            continue;
        }

        // Convert string content to array format and prepend reminder
        let content = &msg["content"];
        if content.is_string() {
            let text = content.as_str().unwrap_or("").to_string();
            msg["content"] = serde_json::json!([
                reminder,
                {"type": "text", "text": text}
            ]);
        } else if content.is_array()
            && let Some(arr) = msg["content"].as_array_mut()
        {
            arr.insert(0, reminder);
        }
        return; // Only inject into first eligible user message
    }

    // No eligible user message found (all are tool_results, e.g. during tool loop).
    // Prepend a standalone user message with just the reminder at position 0.
    messages.insert(
        0,
        serde_json::json!({
            "role": "user",
            "content": [reminder]
        }),
    );
    // Must follow with a minimal assistant ack to maintain user/assistant alternation.
    messages.insert(
        1,
        serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "ok"}]
        }),
    );
}

/// Ensure strict user/assistant message alternation as required by the API.
/// - Consecutive text-only user messages are merged into one.
/// - Between a `tool_result` user message and a text user message, a placeholder
///   assistant message is inserted (can't merge these — `tool_result` + text mixing
///   breaks `inject_system_reminder` and API validation).
/// - Consecutive assistant messages are merged.
fn ensure_message_alternation(messages: &mut Vec<Value>) {
    if messages.len() <= 1 {
        return;
    }

    let mut result: Vec<Value> = Vec::with_capacity(messages.len());

    for msg in messages.drain(..) {
        let same_role = result.last().is_some_and(|last: &Value| last["role"] == msg["role"]);
        if !same_role {
            let blocks = content_to_blocks(&msg["content"]);
            result.push(serde_json::json!({"role": msg["role"], "content": blocks}));
            continue;
        }

        let prev_has_tool_result = result.last().is_some_and(|last| {
            last["content"].as_array().is_some_and(|arr| arr.iter().any(|b| b["type"] == "tool_result"))
        });
        let curr_has_tool_result =
            msg["content"].as_array().is_some_and(|arr| arr.iter().any(|b| b["type"] == "tool_result"));

        if prev_has_tool_result == curr_has_tool_result {
            // Same content type — safe to merge
            let new_blocks = content_to_blocks(&msg["content"]);
            if let Some(arr) = result.last_mut().and_then(|last| last["content"].as_array_mut()) {
                arr.extend(new_blocks);
            }
        } else {
            // Different content types — insert placeholder assistant to separate them
            result.push(serde_json::json!({
                "role": "assistant",
                "content": [{"type": "text", "text": "ok"}]
            }));
            let blocks = content_to_blocks(&msg["content"]);
            result.push(serde_json::json!({"role": msg["role"], "content": blocks}));
        }
    }

    // API requires first message to be user role. Panel injection starts with
    // assistant messages, so prepend a placeholder user message if needed.
    if result.first().is_some_and(|m| m["role"] == "assistant") {
        result.insert(
            0,
            serde_json::json!({
                "role": "user",
                "content": [{"type": "text", "text": "ok"}]
            }),
        );
    }

    *messages = result;
}

/// Convert content (string or array) to an array of content blocks.
fn content_to_blocks(content: &Value) -> Vec<Value> {
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
    let _r = fs::create_dir_all(LAST_REQUESTS_DIR);
    let path = format!("{LAST_REQUESTS_DIR}/{worker_id}_last_request.json");
    let _r = fs::write(path, serde_json::to_string_pretty(&debug).unwrap_or_default());
}

/// Claude Code OAuth client
pub(crate) struct ClaudeCodeClient {
    access_token: Option<SecretBox<String>>,
}

#[derive(Deserialize)]
struct CredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: OAuthCredentials,
}

#[derive(Deserialize)]
struct OAuthCredentials {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "expiresAt")]
    expires_at: u64,
}

impl ClaudeCodeClient {
    pub(crate) fn new() -> Self {
        let access_token = Self::load_oauth_token();
        Self { access_token }
    }
    fn load_oauth_token() -> Option<SecretBox<String>> {
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
        let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_millis().to_u64();

        if now_ms > creds.claude_ai_oauth.expires_at {
            return None; // Token expired
        }

        Some(SecretBox::new(Box::new(creds.claude_ai_oauth.access_token)))
    }

    pub(crate) fn do_check_api(&self, model: &str) -> ApiCheckResult {
        let access_token = match self.access_token.as_ref() {
            Some(t) => t.expose_secret(),
            None => {
                return ApiCheckResult {
                    auth_ok: false,
                    streaming_ok: false,
                    tools_ok: false,
                    error: Some("OAuth token not found or expired".to_string()),
                };
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
            .header("authorization", format!("Bearer {access_token}"))
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

        let auth_ok = auth_result.as_ref().is_ok_and(|resp| resp.status().is_success());

        if !auth_ok {
            let error = auth_result.err().map(|e| e.to_string()).or_else(|| Some("Auth failed".to_string()));
            return ApiCheckResult { auth_ok: false, streaming_ok: false, tools_ok: false, error };
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
            .header("authorization", format!("Bearer {access_token}"))
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
            .header("authorization", format!("Bearer {access_token}"))
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

        ApiCheckResult { auth_ok, streaming_ok, tools_ok, error: None }
    }

    pub(crate) fn do_stream(&self, request: &LlmRequest, tx: &Sender<StreamEvent>) -> Result<(), LlmError> {
        let access_token = self
            .access_token
            .as_ref()
            .ok_or_else(|| LlmError::Auth("Claude Code OAuth token not found or expired. Run 'claude login'".into()))?;

        let client = Client::builder().timeout(None).build().map_err(|e| LlmError::Network(e.to_string()))?;

        // Handle cleaner mode or custom system prompt
        let system_text =
            request.system_prompt.as_ref().map_or_else(|| library::default_agent_content().to_string(), Clone::clone);

        // Build messages from pre-assembled API messages or raw data
        let mut json_messages = if request.api_messages.is_empty() {
            Vec::new()
        } else {
            super::api_messages_to_cc_json(&request.api_messages)
        };

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

        // Ensure strict user/assistant alternation (merges consecutive same-role messages)
        ensure_message_alternation(&mut json_messages);

        // Inject system-reminder into first user message for Claude Code validation
        inject_system_reminder(&mut json_messages);

        // Build final request (cache_control breakpoints are on panel tool_results above)
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

        // Always dump last request for debugging (overwritten each call)
        dump_last_request(&request.worker_id, &api_request);

        let response = client
            .post(CLAUDE_CODE_ENDPOINT)
            .header("accept", "text/event-stream")
            .header("authorization", format!("Bearer {}", access_token.expose_secret()))
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
            .send()?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().unwrap_or_default();
            return Err(LlmError::Api { status, body });
        }

        // Log response headers for debugging stream errors
        let resp_headers: String = response
            .headers()
            .iter()
            .map(|(k, v)| format!("  {}: {}", k, v.to_str().unwrap_or("<binary>")))
            .collect::<Vec<_>>()
            .join("\n");

        let mut reader = BufReader::new(response);
        let mut input_tokens = 0;
        let mut output_tokens = 0;
        let mut cache_hit_tokens = 0;
        let mut cache_miss_tokens = 0;
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
                    total_bytes += n;
                    line_count += 1;
                }
                Err(e) => {
                    // Walk error source chain. Known causes: TimedOut, ConnectionReset, UnexpectedEof
                    let error_kind = format!("{:?}", e.kind());
                    let mut root_cause = String::new();
                    let mut source: Option<&dyn std::error::Error> = std::error::Error::source(&e);
                    while let Some(s) = source {
                        root_cause = format!("{s}");
                        source = std::error::Error::source(s);
                    }
                    let tool_ctx = match &current_tool {
                        Some((id, name, partial)) => {
                            format!("In-flight tool: {} (id={}), partial input: {} bytes", name, id, partial.len())
                        }
                        None => "No tool in progress".to_string(),
                    };
                    let recent =
                        if last_lines.is_empty() { "(no lines read)".to_string() } else { last_lines.join("\n") };
                    let verbose = format!(
                        "{}\n\
                         Error kind: {} | Root cause: {}\n\
                         Stream position: {} bytes, {} lines read\n\
                         {}\n\
                         Response headers:\n{}\n\
                         Last SSE lines:\n{}",
                        e,
                        error_kind,
                        if root_cause.is_empty() { "(none)".to_string() } else { root_cause },
                        total_bytes,
                        line_count,
                        tool_ctx,
                        resp_headers,
                        recent
                    );
                    return Err(LlmError::StreamRead(verbose));
                }
            }
            let line = line.trim_end_matches('\n').trim_end_matches('\r');

            if !line.starts_with("data: ") {
                continue;
            }

            // Keep last 5 data lines for error context
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
                    "message_start" => {
                        if let Some(msg_body) = event.message
                            && let Some(usage) = msg_body.usage
                        {
                            if let Some(hit) = usage.cache_read {
                                cache_hit_tokens = hit;
                            }
                            if let Some(miss) = usage.cache_creation {
                                cache_miss_tokens = miss;
                            }
                            if let Some(inp) = usage.input {
                                input_tokens = inp;
                            }
                        }
                    }
                    "message_delta" => {
                        if let Some(ref delta) = event.delta
                            && let Some(ref reason) = delta.stop_reason
                        {
                            stop_reason = Some(reason.clone());
                        }
                        if let Some(usage) = event.usage {
                            if let Some(inp) = usage.input {
                                input_tokens = inp;
                            }
                            if let Some(out) = usage.output {
                                output_tokens = out;
                            }
                        }
                    }
                    "message_stop" => break,
                    "error" => {
                        crate::llms::log_sse_error(&crate::llms::SseErrorContext {
                            provider: "claude_code",
                            json_str,
                            total_bytes,
                            line_count,
                            last_lines: &last_lines,
                        });
                        break;
                    }
                    _ => {}
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
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamMessageBody {
    usage: Option<StreamUsage>,
}

#[derive(Debug, Deserialize)]
struct StreamMessage {
    #[serde(rename = "type")]
    event_type: String,
    content_block: Option<StreamContentBlock>,
    delta: Option<StreamDelta>,
    usage: Option<StreamUsage>,
    message: Option<StreamMessageBody>,
}

#[derive(Debug, Deserialize)]
struct StreamUsage {
    #[serde(rename = "input_tokens")]
    input: Option<usize>,
    #[serde(rename = "output_tokens")]
    output: Option<usize>,
    #[serde(rename = "cache_creation_input_tokens")]
    cache_creation: Option<usize>,
    #[serde(rename = "cache_read_input_tokens")]
    cache_read: Option<usize>,
}

impl LlmClient for ClaudeCodeClient {
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), LlmError> {
        self.do_stream(&request, &tx)
    }

    fn check_api(&self, model: &str) -> ApiCheckResult {
        self.do_check_api(model)
    }
}
