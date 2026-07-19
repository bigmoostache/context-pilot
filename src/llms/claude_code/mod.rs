//! Claude Code OAuth API implementation.
//!
//! Uses OAuth tokens loaded via the [`cp_vault`] credential vault (macOS
//! Keychain or `~/.claude/.credentials.json`), with Bearer authentication.
//! Replicates Claude Code's request signature to access Claude 4.5 models.

mod check_api;
mod debug;
mod message_format;
mod stream_types;

use std::io::{BufRead as _, BufReader};
use std::sync::mpsc::Sender;

use cp_mod_utilities::secret::Redacted;
use reqwest::blocking::Client;
use serde_json::Value;

use super::error::LlmError;
use super::{ApiCheckResult, LlmClient, LlmRequest, StreamEvent};
use crate::infra::constants::{API_VERSION, library};
use crate::infra::tools::{ToolUse, build_api};
use cp_base::config::INJECTIONS;
use stream_types::StreamMessage;

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

/// Claude Code OAuth client
pub(crate) struct ClaudeCodeClient {
    /// OAuth access token loaded from the vault (Keychain or `~/.claude/.credentials.json`)
    access_token: Option<Redacted>,
}

impl ClaudeCodeClient {
    /// Create a new Claude Code client, loading the OAuth token from the vault.
    ///
    /// Resolves through the [`cp_vault::vault()`] singleton — the single entry
    /// point for every credential — so the full cascade applies (in-memory
    /// overrides → Keychain → credential file). This is the SAME path the
    /// orchestrator uses for the usage proxy, guaranteeing agent and
    /// orchestrator always see one identical token source.
    pub(crate) fn new() -> Self {
        let access_token = cp_vault::vault().get("claude_oauth").map(|s| Redacted::new(s.expose().to_owned()));
        Self { access_token }
    }

    /// Run sequential API health checks: auth, streaming, and tool calling.
    pub(crate) fn do_check_api(&self, model: &str) -> ApiCheckResult {
        let access_token = match self.access_token.as_ref() {
            Some(t) => t.expose_secret(),
            None => {
                return ApiCheckResult {
                    auth_ok: false,
                    streaming_ok: false,
                    tools_ok: false,
                    error: Some("OAuth token not found or expired".to_owned()),
                };
            }
        };

        let client = Client::new();
        let mapped_model = map_model_name(model);
        let system = check_api::system_block();

        // Test 1: Basic auth — simple non-streaming request
        let auth_result = check_api::build_check_request(&check_api::CheckRequest {
            client: &client,
            access_token,
            model: mapped_model,
            system: &system,
            user_text: "Hi",
            stream: false,
            tools: None,
        })
        .send();
        let auth_ok = auth_result.as_ref().is_ok_and(|r| r.status().is_success());
        if !auth_ok {
            let error = auth_result.err().map(|e| e.to_string()).or_else(|| Some("Auth failed".to_owned()));
            return ApiCheckResult { auth_ok: false, streaming_ok: false, tools_ok: false, error };
        }

        // Test 2: Streaming
        let stream_result = check_api::build_check_request(&check_api::CheckRequest {
            client: &client,
            access_token,
            model: mapped_model,
            system: &system,
            user_text: "Say ok",
            stream: true,
            tools: None,
        })
        .send();
        let streaming_ok = stream_result.as_ref().is_ok_and(|r| r.status().is_success());

        // Test 3: Tool calling
        let test_tool = serde_json::json!([{
            "name": "test_tool",
            "description": "A test tool",
            "input_schema": {"type": "object", "properties": {}, "required": []}
        }]);
        let tools_result = check_api::build_check_request(&check_api::CheckRequest {
            client: &client,
            access_token,
            model: mapped_model,
            system: &system,
            user_text: "Hi",
            stream: false,
            tools: Some(&test_tool),
        })
        .send();
        let tools_ok = tools_result.as_ref().is_ok_and(|r| r.status().is_success());

        ApiCheckResult { auth_ok, streaming_ok, tools_ok, error: None }
    }

    /// Execute a streaming request against the Claude Code API.
    pub(crate) fn do_stream(&self, request: &LlmRequest, tx: &Sender<StreamEvent>) -> Result<(), LlmError> {
        let access_token = self
            .access_token
            .as_ref()
            .ok_or_else(|| LlmError::Auth("Claude Code OAuth token not found or expired. Run 'claude login'".into()))?;

        let client = Client::builder().timeout(None).build().map_err(|e| LlmError::Network(e.to_string()))?;

        // Handle cleaner mode or custom system prompt
        let system_text =
            request.system_prompt.as_ref().map_or_else(|| library::default_agent_content().to_owned(), Clone::clone);

        // Build messages from pre-assembled API messages or raw data
        let super::CcJsonResult { mut json_messages, bp_hashes, bp_panel_ids, alive_count, alive_positions_permille } =
            if request.api_messages.is_empty() {
                super::CcJsonResult {
                    json_messages: Vec::new(),
                    bp_hashes: Vec::new(),
                    bp_panel_ids: Vec::new(),
                    alive_count: 0,
                    alive_positions_permille: Vec::new(),
                }
            } else {
                super::api_messages_to_cc_json(&request.api_messages, request.cache_engine_json.as_deref())
            };

        // Handle cleaner mode extra context
        if let Some(context) = &(request.extra_context) {
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

        // System-reminder injection for Claude Code validation
        message_format::inject_system_reminder(&mut json_messages);

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
        debug::dump_last_request(&request.worker_id, &api_request);

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
                    total_bytes = total_bytes.saturating_add(n);
                    line_count = line_count.saturating_add(1);
                }
                Err(e) => {
                    let verbose = debug::build_stream_read_error(&debug::StreamErrorContext {
                        err: &e,
                        current_tool: current_tool.as_ref(),
                        total_bytes,
                        line_count,
                        resp_headers: &resp_headers,
                        last_lines: &last_lines,
                    });
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
            last_lines.push(line.to_owned());

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
                                        && let Some((_, name, input)) = current_tool.as_mut()
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
                        if let Some(delta) = &event.delta
                            && let Some(reason) = &delta.stop_reason
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
            bp_hashes,
            bp_panel_ids,
            alive_count,
            alive_positions_permille,
        });
        Ok(())
    }
}

impl Default for ClaudeCodeClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmClient for ClaudeCodeClient {
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), LlmError> {
        self.do_stream(&request, &tx)
    }

    fn check_api(&self, model: &str) -> ApiCheckResult {
        self.do_check_api(model)
    }
}
