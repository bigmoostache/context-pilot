//! Claude Code OAuth API implementation.
//!
//! Uses OAuth tokens loaded via the [`cp_vault`] credential vault (macOS
//! Keychain or `~/.claude/.credentials.json`), with Bearer authentication.
//! Replicates Claude Code's request signature to access Claude 4.5 models.

mod check_api;
mod debug;
mod message_format;
mod stream_types;
mod streaming;

use std::sync::mpsc::Sender;

use cp_mod_utilities::secret::Redacted;
use reqwest::blocking::Client;

use super::error::LlmError;
use super::{ApiCheckResult, LlmClient, LlmRequest, StreamEvent};
use crate::infra::constants::{API_VERSION, library};
use crate::infra::tools::build_api;
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

/// POST an assembled request body to the Claude Code endpoint with the full
/// CLI header signature (Bearer OAuth, beta flags, stainless UA). Extracted so
/// `do_stream` stays under the line cap; the header set is the OAuth-CLI
/// contract required for Claude 4.5 access.
fn send_cc_request(
    client: &Client,
    access_token: &str,
    api_request: &serde_json::Value,
) -> Result<reqwest::blocking::Response, LlmError> {
    client
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
        .json(api_request)
        .send()
        .map_err(LlmError::from)
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
                return ApiCheckResult::failure(Some("OAuth token not found or expired".to_owned()));
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
            return ApiCheckResult::failure(error);
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

        ApiCheckResult::checks([auth_ok, streaming_ok, tools_ok])
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

        // Build messages (api-message conversion + cleaner context + tool results)
        let super::CcJsonResult { mut json_messages, bp_hashes, bp_panel_ids, alive_count, alive_positions_permille } =
            super::claude_code_api_key::helpers::build_cc_json_messages(request);

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

        let response = send_cc_request(&client, access_token.expose_secret(), &api_request)?;

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

        let totals = streaming::consume_cc_stream(response, &resp_headers, tx)?;

        let _r = tx.send(StreamEvent::Done {
            input_tokens: totals.input_tokens,
            output_tokens: totals.output_tokens,
            cache_hit_tokens: totals.cache_hit_tokens,
            cache_miss_tokens: totals.cache_miss_tokens,
            stop_reason: totals.stop_reason,
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
