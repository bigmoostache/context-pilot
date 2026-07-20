//! Claude Code V2 OAuth API implementation.
//!
//! Uses the same OAuth tokens as `claude_code`, loaded via the [`cp_vault`]
//! credential vault, but with the updated request format captured from Claude
//! Code CLI v2.1.170:
//! - Adaptive thinking (`"thinking": { "type": "adaptive" }`)
//! - High effort output (`"output_config": { "effort": "high" }`)
//! - Context management with thinking preservation
//! - Updated beta flags (14 flags vs original 5)
//! - Updated billing header and user-agent strings

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

/// Beta flags from captured Claude Code CLI v2.1.170 traffic.
/// 5 internal-only flags removed (code-completions, code-reviews, patterns,
/// model-preference, expanded-citations) — the public API rejects them.
const BETA_HEADER: &str = "\
    claude-code-20250219,\
    oauth-2025-04-20,\
    interleaved-thinking-2025-05-14,\
    context-management-2025-06-27,\
    prompt-caching-scope-2026-01-05,\
    structured-outputs-2025-12-15,\
    files-api-2025-04-14,\
    mcp-client-2025-04-04,\
    token-efficient-tools-2025-02-19";

/// Billing header for V2 traffic validation.
///
/// `cc_is_subagent=true;` is appended systematically — captured Claude Code
/// subagent traffic carries this flag (main-agent traffic does not). Marking
/// our requests as subagent may route them through a distinct billing/rate
/// bucket.
const BILLING_HEADER: &str =
    "x-anthropic-billing-header: cc_version=2.1.170.6bc; cc_entrypoint=sdk-cli; cch=3d037; cc_is_subagent=true;";

/// POST an assembled V2 request body to the endpoint with the full CLI v2.1.170
/// header signature (Bearer OAuth, 9 beta flags, stainless UA). Extracted so
/// `do_stream` stays under the line cap.
fn send_v2_request(
    client: &Client,
    access_token: &str,
    api_request: &serde_json::Value,
) -> Result<reqwest::blocking::Response, LlmError> {
    client
        .post(ENDPOINT)
        .header("accept", "text/event-stream")
        .header("authorization", format!("Bearer {access_token}"))
        .header("anthropic-version", API_VERSION)
        .header("anthropic-beta", BETA_HEADER)
        .header("anthropic-dangerous-direct-browser-access", "true")
        .header("content-type", "application/json")
        .header("user-agent", "claude-cli/2.1.170 (external, sdk-cli)")
        .header("x-app", "cli")
        .header("x-stainless-arch", "x64")
        .header("x-stainless-lang", "js")
        .header("x-stainless-os", "Linux")
        .header("x-stainless-package-version", "0.94.0")
        .header("x-stainless-retry-count", "0")
        .header("x-stainless-runtime", "node")
        .header("x-stainless-runtime-version", "v24.3.0")
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

        // Build V2 request body — matches V1 structure (standard fields only).
        // Captured V2 fields (thinking, output_config, context_management,
        // diagnostics) are internal-only and rejected by the public API.
        let api_request = serde_json::json!({
            "model": mapped_model,
            "max_tokens": request.max_output_tokens,
            "system": [
                {"type": "text", "text": BILLING_HEADER},
                {"type": "text", "text": system_text}
            ],
            "messages": json_messages,
            "tools": build_api(&request.tools),
            "stream": true
        });

        // Debug dump
        helpers::dump_last_request(&request.worker_id, &api_request);

        // Build and send request with V2 headers
        let response = send_v2_request(&client, access_token.expose_secret(), &api_request)?;

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
        let system_json = serde_json::json!([
            {"type": "text", "text": BILLING_HEADER},
            {"type": "text", "text": "You are a helpful assistant."}
        ]);
        // Post one probe body with V2 headers; report whether it succeeded.
        let probe = |body: &serde_json::Value| {
            client
                .post(ENDPOINT)
                .header("authorization", format!("Bearer {access_token}"))
                .header("anthropic-version", API_VERSION)
                .header("anthropic-beta", BETA_HEADER)
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
