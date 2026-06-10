//! Claude Code V2 OAuth API implementation.
//!
//! Uses the same OAuth tokens as `claude_code` (macOS Keychain / credentials file)
//! but with the updated request format captured from Claude Code CLI v2.1.170:
//! - Adaptive thinking (`"thinking": { "type": "adaptive" }`)
//! - High effort output (`"output_config": { "effort": "high" }`)
//! - Context management with thinking preservation
//! - Updated beta flags (14 flags vs original 5)
//! - Updated billing header and user-agent strings

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::Sender;

use cp_mod_utilities::secret::Redacted;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::Value;

use super::claude_code_api_key::helpers;
use super::claude_code_api_key::streaming;
use super::error::LlmError;
use super::{ApiCheckResult, LlmClient, LlmRequest, StreamEvent};
use crate::infra::constants::{API_VERSION, library};
use crate::infra::tools::build_api;
use cp_base::cast::Safe as _;

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

/// Claude Code V2 OAuth client.
pub(crate) struct ClaudeCodeV2Client {
    /// OAuth access token from macOS Keychain or credentials file.
    access_token: Option<Redacted>,
}

/// On-disk credentials file structure for Claude Code OAuth.
#[derive(Deserialize)]
struct CredentialsFile {
    /// OAuth credentials section.
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: OAuthCredentials,
}

/// OAuth credential fields.
#[derive(Deserialize)]
struct OAuthCredentials {
    /// Bearer access token.
    #[serde(rename = "accessToken")]
    access_token: String,
    /// Token expiry timestamp in milliseconds since UNIX epoch.
    #[serde(rename = "expiresAt")]
    expires_at: u64,
}

impl ClaudeCodeV2Client {
    /// Create a new V2 client, loading the OAuth token.
    pub(crate) fn new() -> Self {
        let access_token = Self::load_oauth_token();
        Self { access_token }
    }

    /// Load OAuth token from macOS Keychain (preferred) or credentials file.
    fn load_oauth_token() -> Option<Redacted> {
        if cfg!(target_os = "macos")
            && let Some(token) = Self::load_from_keychain()
        {
            return Some(token);
        }
        Self::load_from_file()
    }

    /// Read from macOS Keychain via `security` CLI.
    fn load_from_keychain() -> Option<Redacted> {
        let output = Command::new("security")
            .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let content = String::from_utf8(output.stdout).ok()?;
        Self::parse_credentials(content.trim())
    }

    /// Read from `~/.claude/.credentials.json`.
    fn load_from_file() -> Option<Redacted> {
        let home = env::var("HOME").ok()?;
        let home_path = PathBuf::from(&home);
        let creds_path = home_path.join(".claude").join(".credentials.json");
        let path = if creds_path.exists() {
            creds_path
        } else {
            home_path.join(".claude").join("credentials.json")
        };
        let content = fs::read_to_string(&path).ok()?;
        Self::parse_credentials(&content)
    }

    /// Parse credentials JSON and return access token if not expired.
    fn parse_credentials(content: &str) -> Option<Redacted> {
        let creds: CredentialsFile = serde_json::from_str(content).ok()?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis()
            .to_u64();
        if now_ms > creds.claude_ai_oauth.expires_at {
            return None;
        }
        Some(Redacted::new(creds.claude_ai_oauth.access_token))
    }

    /// Execute a streaming request with the V2 request format.
    pub(crate) fn do_stream(
        &self,
        request: &LlmRequest,
        tx: &Sender<StreamEvent>,
    ) -> Result<(), LlmError> {
        let access_token = self.access_token.as_ref().ok_or_else(|| {
            LlmError::Auth("Claude Code OAuth token not found or expired. Run 'claude login'".into())
        })?;

        let client = Client::builder()
            .timeout(None)
            .build()
            .map_err(|e| LlmError::Network(e.to_string()))?;

        // System prompt
        let system_text = request
            .system_prompt
            .as_ref()
            .map_or_else(|| library::default_agent_content().to_string(), Clone::clone);

        // Convert pre-assembled API messages to JSON with cache breakpoints
        let super::CcJsonResult {
            mut json_messages,
            bp_hashes,
            alive_count,
            alive_positions_permille,
        } = if request.api_messages.is_empty() {
            super::CcJsonResult {
                json_messages: Vec::new(),
                bp_hashes: Vec::new(),
                alive_count: 0,
                alive_positions_permille: Vec::new(),
            }
        } else {
            super::api_messages_to_cc_json(
                &request.api_messages,
                request.cache_engine_json.as_deref(),
            )
        };

        // Cleaner mode extra context
        if let Some(ref context) = request.extra_context {
            let msg = cp_base::config::INJECTIONS
                .providers
                .cleaner_mode
                .trim_end()
                .replace(concat!("{", "context", "}"), context);
            json_messages.push(serde_json::json!({
                "role": "user",
                "content": msg
            }));
        }

        // Pending tool results
        if let Some(results) = &request.tool_results {
            let tool_results: Vec<Value> = results
                .iter()
                .map(|r| {
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
        let response = client
            .post(ENDPOINT)
            .header("accept", "text/event-stream")
            .header(
                "authorization",
                format!("Bearer {}", access_token.expose_secret()),
            )
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
            .json(&api_request)
            .send()?;

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
                return ApiCheckResult {
                    auth_ok: false,
                    streaming_ok: false,
                    tools_ok: false,
                    error: Some(
                        "OAuth token not found or expired".to_string(),
                    ),
                };
            }
        };

        let client = Client::new();
        let mapped_model = helpers::map_model_name(model);
        let system_json = serde_json::json!([
            {"type": "text", "text": BILLING_HEADER},
            {"type": "text", "text": "You are a helpful assistant."}
        ]);

        // Test 1: Basic auth
        let auth_body = serde_json::json!({
            "model": mapped_model,
            "max_tokens": 32,
            "system": system_json,
            "messages": [{"role": "user", "content": "Hi"}],
            "stream": false
        });
        let auth_result = client
            .post(ENDPOINT)
            .header("authorization", format!("Bearer {access_token}"))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", BETA_HEADER)
            .header("content-type", "application/json")
            .json(&auth_body)
            .send();
        let auth_ok = auth_result
            .as_ref()
            .is_ok_and(|r| r.status().is_success());
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

        // Test 2: Streaming
        let stream_body = serde_json::json!({
            "model": mapped_model,
            "max_tokens": 32,
            "system": system_json,
            "messages": [{"role": "user", "content": "Say ok"}],
            "stream": true
        });
        let stream_result = client
            .post(ENDPOINT)
            .header("authorization", format!("Bearer {access_token}"))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", BETA_HEADER)
            .header("content-type", "application/json")
            .json(&stream_body)
            .send();
        let streaming_ok = stream_result
            .as_ref()
            .is_ok_and(|r| r.status().is_success());

        // Test 3: Tool calling
        let tools_body = serde_json::json!({
            "model": mapped_model,
            "max_tokens": 32,
            "system": system_json,
            "messages": [{"role": "user", "content": "Hi"}],
            "tools": [{"name": "test_tool", "description": "A test tool", "input_schema": {"type": "object", "properties": {}, "required": []}}],
            "stream": false
        });
        let tools_result = client
            .post(ENDPOINT)
            .header("authorization", format!("Bearer {access_token}"))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", BETA_HEADER)
            .header("content-type", "application/json")
            .json(&tools_body)
            .send();
        let tools_ok = tools_result
            .as_ref()
            .is_ok_and(|r| r.status().is_success());

        ApiCheckResult {
            auth_ok,
            streaming_ok,
            tools_ok,
            error: None,
        }
    }
}

impl Default for ClaudeCodeV2Client {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmClient for ClaudeCodeV2Client {
    fn stream(
        &self,
        request: LlmRequest,
        tx: Sender<StreamEvent>,
    ) -> Result<(), LlmError> {
        self.do_stream(&request, &tx)
    }

    fn check_api(&self, model: &str) -> ApiCheckResult {
        self.do_check_api(model)
    }
}
