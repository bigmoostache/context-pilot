//! Token exchange, refresh, and persistent storage for OAuth credentials.
//!
//! Stores tokens per-server in `~/.context-pilot/mcp-tokens.json`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};

use crate::errors::McpError;

use super::urlencoded;

/// Token file relative to the global config directory.
const TOKENS_FILE: &str = "mcp-tokens.json";

/// Safety margin (seconds) subtracted from `expires_at` to avoid clock-skew
/// races. A stored token is considered expired 60 s before its real deadline.
const EXPIRY_MARGIN_SECS: u64 = 60;

// ── Token Exchange ──────────────────────────────────────────────────────────

/// Successful token-endpoint response.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenResponse {
    /// Bearer access token.
    pub access_token: String,
    /// Token type (usually `"Bearer"`).
    #[serde(default)]
    pub token_type: String,
    /// Lifetime in seconds from issuance; absent means no expiry.
    #[serde(default)]
    pub expires_in: Option<u64>,
    /// Refresh token for obtaining new access tokens.
    #[serde(default)]
    pub refresh_token: Option<String>,
}

/// Exchange an authorization code for tokens at the token endpoint.
///
/// # Errors
///
/// Returns transport or protocol errors.
pub fn exchange_code(
    metadata: &super::OAuthMetadata,
    code: &str,
    params: &super::AuthFlowParams<'_>,
) -> Result<TokenResponse, McpError> {
    let body = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
        urlencoded(code),
        urlencoded(params.redirect_uri),
        urlencoded(params.client_id),
        urlencoded(&params.pkce.verifier),
    );
    post_token_request(&metadata.token_exchange, &body)
}

/// Refresh an access token using a stored refresh token.
///
/// # Errors
///
/// Returns transport or protocol errors.
pub fn refresh_access_token(
    token_endpoint: &str,
    refresh_token: &str,
    client_id: &str,
) -> Result<TokenResponse, McpError> {
    let body = format!(
        "grant_type=refresh_token&refresh_token={}&client_id={}",
        urlencoded(refresh_token),
        urlencoded(client_id),
    );
    post_token_request(token_endpoint, &body)
}

/// POST a form-encoded body to a token endpoint and decode the response.
fn post_token_request(url: &str, body: &str) -> Result<TokenResponse, McpError> {
    let resp = Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| McpError::Transport(format!("build HTTP client: {e}")))?
        .post(url)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(body.to_owned())
        .send()
        .map_err(|e| McpError::Transport(format!("token exchange: {e}")))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        let snippet: String = text.chars().take(200).collect();
        return Err(McpError::Protocol(format!(
            "token endpoint returned {status}: {snippet}"
        )));
    }
    resp.json::<TokenResponse>()
        .map_err(|e| McpError::Protocol(format!("decode token response: {e}")))
}

// ── Token Storage ───────────────────────────────────────────────────────────

/// On-disk token record for one server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredToken {
    /// Bearer access token.
    pub access_token: String,
    /// Refresh token for renewal.
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Unix timestamp (seconds) at which the access token expires.
    #[serde(default)]
    pub expires_at: Option<u64>,
    /// Client ID used during authorization (needed for refresh).
    #[serde(default)]
    pub client_id: Option<String>,
    /// Token endpoint (needed for refresh without re-discovery).
    #[serde(default)]
    pub token_endpoint: Option<String>,
}

/// Path to the global token store: `~/.context-pilot/mcp-tokens.json`.
#[must_use]
pub fn store_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".context-pilot").join(TOKENS_FILE))
}

/// Load all stored tokens, or an empty map if the file is absent / corrupt.
#[must_use]
pub fn load_all() -> HashMap<String, StoredToken> {
    let Some(path) = store_path() else { return HashMap::new() };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Load a stored token for a server key, returning `None` if absent or expired.
#[must_use]
pub fn load_stored_token(server_key: &str) -> Option<String> {
    let tokens = load_all();
    let stored = tokens.get(server_key)?;
    if let Some(exp) = stored.expires_at {
        let now = now_secs();
        if now.saturating_add(EXPIRY_MARGIN_SECS) >= exp {
            return None; // Expired (or about to).
        }
    }
    Some(stored.access_token.clone())
}

/// Store a token from a fresh [`TokenResponse`], keyed by server.
///
/// # Errors
///
/// Returns [`McpError::Transport`] on file I/O failure.
pub fn store_token(
    server_key: &str,
    resp: &TokenResponse,
    client_id: &str,
    token_endpoint: &str,
) -> Result<(), McpError> {
    let expires_at = resp.expires_in.map(|secs| now_secs().saturating_add(secs));
    let entry = StoredToken {
        access_token: resp.access_token.clone(),
        refresh_token: resp.refresh_token.clone(),
        expires_at,
        client_id: Some(client_id.to_owned()),
        token_endpoint: Some(token_endpoint.to_owned()),
    };
    let mut tokens = load_all();
    let _prev = tokens.insert(server_key.to_owned(), entry);
    let Some(path) = store_path() else {
        return Err(McpError::Transport("cannot resolve token store path".to_owned()));
    };
    if let Some(parent) = path.parent() {
        let _dir = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(&tokens)
        .map_err(|e| McpError::Protocol(format!("encode tokens: {e}")))?;
    std::fs::write(&path, json)
        .map_err(|e| McpError::Transport(format!("write {}: {e}", path.display())))
}

/// Current wall-clock time as Unix seconds.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
