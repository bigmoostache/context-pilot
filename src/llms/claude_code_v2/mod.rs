//! Claude Code V2 OAuth API implementation.
//!
//! Uses OAuth tokens loaded via the [`cp_vault`] credential vault, with the
//! request signature captured live from Claude Code CLI v2.1.220:
//! - Endpoint `/v1/messages?beta=true`, Bearer-OAuth auth (no `x-api-key`).
//! - 13 beta flags led by `oauth-2025-04-20` (selects the Bearer path);
//!   `context-1m-2025-08-07` stripped per-model for Haiku (subscription rejects it).
//! - First system block is the Claude Code identity — OAuth tokens are gated to
//!   Claude-Code-shaped requests.
//! - Stainless user-agent `claude-cli/2.1.220 (external, sdk-cli)`, macOS/arm64.
//!
//! Reuses `helpers` + `parse_sse_stream` from `claude_code_api_key`.

use std::sync::mpsc::Sender;

use cp_mod_utilities::secret::Redacted;
use reqwest::blocking::Client;

use super::claude_code_api_key::helpers;
use super::claude_code_api_key::streaming;
use super::error::LlmError;
use super::{ApiCheckResult, LlmClient, LlmRequest, StreamEvent};
use crate::infra::constants::{API_VERSION, library};
use crate::infra::tools::build_api;

/// API endpoint with beta query parameter.
const ENDPOINT: &str = "https://api.anthropic.com/v1/messages?beta=true";

/// Base beta flags captured live from Claude Code CLI v2.1.220 (main-agent set).
///
/// `oauth-2025-04-20` MUST lead: it selects the Bearer-OAuth auth path. Without
/// it the server falls back to `x-api-key` and rejects the request
/// (`invalid x-api-key`). Every flag below was validated against the live
/// `/v1/messages?beta=true` endpoint (200 for opus/sonnet/fable). `context-1m`
/// is stripped per-model for Haiku (see [`beta_header_for`]).
const BETA_FLAGS: &[&str] = &[
    "oauth-2025-04-20",
    "claude-code-20250219",
    "context-1m-2025-08-07",
    "interleaved-thinking-2025-05-14",
    "thinking-token-count-2026-05-13",
    "context-management-2025-06-27",
    "prompt-caching-scope-2026-01-05",
    "mid-conversation-system-2026-04-07",
    "advisor-tool-2026-03-01",
    "advanced-tool-use-2025-11-20",
    "effort-2025-11-24",
    "fallback-credit-2026-06-01",
    "cache-diagnosis-2026-04-07",
];

/// The Claude Code identity system block. OAuth tokens are gated to
/// Claude-Code-shaped requests: the FIRST system block must be this identity
/// text or the API rejects the credential.
const CC_IDENTITY: &str = "You are Claude Code, Anthropic's official CLI for Claude.";

/// Assemble the `anthropic-beta` header for a given API model. Haiku-tier
/// rejects the 1M long-context beta on this subscription
/// (`The long context beta is not yet available for this subscription`), so
/// `context-1m-2025-08-07` is dropped for any `haiku` model; all others keep the
/// full set.
fn beta_header_for(model: &str) -> String {
    let is_haiku = model.contains("haiku");
    BETA_FLAGS
        .iter()
        .filter(|flag| !(is_haiku && **flag == "context-1m-2025-08-07"))
        .copied()
        .collect::<Vec<_>>()
        .join(",")
}

/// POST an assembled V2 request body with the full CLI v2.1.220 header
/// signature (Bearer OAuth, model-aware beta flags, stainless UA). Extracted so
/// `do_stream` stays under the line cap.
fn send_v2_request(
    client: &Client,
    access_token: &str,
    model: &str,
    api_request: &serde_json::Value,
) -> Result<reqwest::blocking::Response, LlmError> {
    client
        .post(ENDPOINT)
        .header("accept", "text/event-stream")
        .header("authorization", format!("Bearer {access_token}"))
        .header("anthropic-version", API_VERSION)
        .header("anthropic-beta", beta_header_for(model))
        .header("anthropic-dangerous-direct-browser-access", "true")
        .header("content-type", "application/json")
        .header("user-agent", "claude-cli/2.1.220 (external, sdk-cli)")
        .header("x-app", "cli")
        .header("x-stainless-arch", "arm64")
        .header("x-stainless-lang", "js")
        .header("x-stainless-os", "MacOS")
        .header("x-stainless-package-version", "0.94.0")
        .header("x-stainless-retry-count", "0")
        .header("x-stainless-runtime", "node")
        .header("x-stainless-runtime-version", "v26.3.0")
        .header("x-stainless-timeout", "600")
        .json(api_request)
        .send()
        .map_err(LlmError::from)
}

/// Claude Code V2 OAuth client.
pub(crate) struct ClaudeCodeV2Client {
    /// OAuth access token from the vault (Keychain or credentials file).
    access_token: Option<Redacted>,
}

impl ClaudeCodeV2Client {
    /// Create a new V2 client, loading the OAuth token from the vault.
    ///
    /// Resolves through the [`cp_vault::vault()`] singleton — the single entry
    /// point for every credential — so the full cascade applies (in-memory
    /// overrides → Keychain → credential file), identical to `claude_code` and
    /// the orchestrator usage proxy.
    pub(crate) fn new() -> Self {
        let access_token = cp_vault::vault().get("claude_oauth").map(|s| Redacted::new(s.expose().to_owned()));
        Self { access_token }
    }

    /// Execute a streaming request with the V2 request format.
    pub(crate) fn do_stream(&self, request: &LlmRequest, tx: &Sender<StreamEvent>) -> Result<(), LlmError> {
        let access_token = self
            .access_token
            .as_ref()
            .ok_or_else(|| LlmError::Auth("Claude Code OAuth token not found or expired. Run 'claude login'".into()))?;

        let client = Client::builder().timeout(None).build().map_err(|e| LlmError::Network(e.to_string()))?;

        // System prompt
        let system_text =
            request.system_prompt.as_ref().map_or_else(|| library::default_agent_content().to_owned(), Clone::clone);

        // Build messages (api-message conversion + cleaner context + tool results)
        let super::CcJsonResult { mut json_messages, bp_hashes, bp_panel_ids, alive_count, alive_positions_permille } =
            helpers::build_cc_json_messages(request);

        // System-reminder injection for Claude Code validation
        helpers::inject_system_reminder(&mut json_messages);

        let mapped_model = helpers::map_model_name(&request.model);

        // Build V2 request body. The first system block MUST be the Claude Code
        // identity — OAuth tokens are gated to Claude-Code-shaped requests.
        let api_request = serde_json::json!({
            "model": mapped_model,
            "max_tokens": request.max_output_tokens,
            "system": [
                {"type": "text", "text": CC_IDENTITY},
                {"type": "text", "text": system_text}
            ],
            "messages": json_messages,
            "tools": build_api(&request.tools),
            "stream": true
        });

        // Debug dump
        helpers::dump_last_request(&request.worker_id, &api_request);

        // Build and send request with V2 headers (beta flags are model-aware)
        let response = send_v2_request(&client, access_token.expose_secret(), mapped_model, &api_request)?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().unwrap_or_default();
            return Err(LlmError::Api { status, body });
        }

        // Capture response headers for SSE error diagnostics
        let resp_headers: String = response
            .headers()
            .iter()
            .map(|(k, v)| format!("  {}: {}", k, v.to_str().unwrap_or("<binary>")))
            .collect::<Vec<_>>()
            .join("\n");

        // Reuse SSE parser from claude_code_api_key
        let (input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, stop_reason) =
            streaming::parse_sse_stream(response, &resp_headers, tx)?;

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

    /// Run API health checks.
    pub(crate) fn do_check_api(&self, model: &str) -> ApiCheckResult {
        let access_token = match self.access_token.as_ref() {
            Some(t) => t.expose_secret(),
            None => {
                return ApiCheckResult::failure(Some("OAuth token not found or expired".to_owned()));
            }
        };

        let client = Client::new();
        let mapped_model = helpers::map_model_name(model);
        let beta = beta_header_for(mapped_model);
        let system_json = serde_json::json!([
            {"type": "text", "text": CC_IDENTITY},
            {"type": "text", "text": "You are a helpful assistant."}
        ]);
        // Post one probe body with V2 headers; report whether it succeeded.
        let probe = |body: &serde_json::Value| {
            client
                .post(ENDPOINT)
                .header("authorization", format!("Bearer {access_token}"))
                .header("anthropic-version", API_VERSION)
                .header("anthropic-beta", &beta)
                .header("content-type", "application/json")
                .json(body)
                .send()
                .is_ok_and(|r| r.status().is_success())
        };

        // Test 1: Basic auth
        let auth_ok = probe(&serde_json::json!({
            "model": mapped_model, "max_tokens": 32i32, "system": system_json,
            "messages": [{"role": "user", "content": "Hi"}], "stream": false
        }));
        if !auth_ok {
            return ApiCheckResult::failure(Some("Auth failed".to_owned()));
        }

        // Test 2: Streaming
        let streaming_ok = probe(&serde_json::json!({
            "model": mapped_model, "max_tokens": 32i32, "system": system_json,
            "messages": [{"role": "user", "content": "Say ok"}], "stream": true
        }));

        // Test 3: Tool calling
        let tools_ok = probe(&serde_json::json!({
            "model": mapped_model, "max_tokens": 32i32, "system": system_json,
            "messages": [{"role": "user", "content": "Hi"}],
            "tools": [{"name": "test_tool", "description": "A test tool", "input_schema": {"type": "object", "properties": {}, "required": []}}],
            "stream": false
        }));

        ApiCheckResult::checks([auth_ok, streaming_ok, tools_ok])
    }
}

impl Default for ClaudeCodeV2Client {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmClient for ClaudeCodeV2Client {
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), LlmError> {
        self.do_stream(&request, &tx)
    }

    fn check_api(&self, model: &str) -> ApiCheckResult {
        self.do_check_api(model)
    }
}
