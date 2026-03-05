//! API health-check helpers for Claude Code OAuth.
//!
//! Three sequential checks: auth -> streaming -> tool calling.
//! The `do_check_api` method lives in the main `impl ClaudeCodeClient`
//! block in `mod.rs` to satisfy the `multiple_inherent_impl` lint.

use reqwest::blocking::Client;
use serde_json::Value;

use super::{BILLING_HEADER, CLAUDE_CODE_ENDPOINT, OAUTH_BETA_HEADER, SYSTEM_REMINDER};
use crate::infra::constants::API_VERSION;

/// System block with billing header required by Claude Code.
pub(super) fn system_block() -> Value {
    serde_json::json!([
        {"type": "text", "text": BILLING_HEADER},
        {"type": "text", "text": "You are a helpful assistant."}
    ])
}

/// Parameters for building a Claude Code health-check request.
pub(super) struct CheckRequest<'req> {
    /// HTTP client for the request.
    pub client: &'req Client,
    /// OAuth access token.
    pub access_token: &'req str,
    /// Model identifier.
    pub model: &'req str,
    /// System message block.
    pub system: &'req Value,
    /// User message text.
    pub user_text: &'req str,
    /// Whether to request SSE streaming.
    pub stream: bool,
    /// Optional tool definitions.
    pub tools: Option<&'req Value>,
}

/// Build a health-check request with standard Claude Code headers.
pub(super) fn build_check_request(req: &CheckRequest<'_>) -> reqwest::blocking::RequestBuilder {
    let user_msg = serde_json::json!({
        "role": "user",
        "content": [
            {"type": "text", "text": SYSTEM_REMINDER},
            {"type": "text", "text": req.user_text}
        ]
    });

    let max_tokens = req.tools.map_or(10, |_| 50);
    let mut body = serde_json::json!({
        "model": req.model,
        "max_tokens": max_tokens,
        "system": req.system,
        "messages": [user_msg]
    });
    if req.stream
        && let Some(obj) = body.as_object_mut()
    {
        let _r = obj.insert("stream".to_string(), serde_json::json!(true));
    }
    if let Some(t) = req.tools
        && let Some(obj) = body.as_object_mut()
    {
        let _r = obj.insert("tools".to_string(), t.clone());
    }

    let accept = if req.stream { "text/event-stream" } else { "application/json" };

    req.client
        .post(CLAUDE_CODE_ENDPOINT)
        .header("accept", accept)
        .header("authorization", format!("Bearer {}", req.access_token))
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
}
