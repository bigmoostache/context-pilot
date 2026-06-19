//! OAuth 2.1 + PKCE authorization flow for remote MCP servers.
//!
//! Implements the MCP spec's recommended auth path: discover OAuth metadata at
//! `{origin}/.well-known/oauth-authorization-server`, run an authorization-code
//! flow with PKCE, and persist tokens in `~/.context-pilot/mcp-tokens.json`.
//!
//! The [`authorize`] entry point opens the user's browser and waits on a
//! transient localhost callback listener. It is deliberately **blocking** — call
//! it from a setup CLI or a background thread, never from `init_state`.

/// PKCE generation, authorization URL building, and localhost callback listener.
pub mod callback;
/// Token exchange, refresh, and persistent storage.
pub mod tokens;

use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;

use crate::errors::McpError;

use self::callback::{AuthFlowParams, build_auth_url, generate_pkce, generate_state, start_listener, wait_for_redirect};
use self::tokens::{exchange_code, load_all, load_stored_token, refresh_access_token, store_token};

// ── OAuth Metadata ──────────────────────────────────────────────────────────

/// Server-advertised OAuth endpoints, fetched from the well-known discovery URL.
#[derive(Debug, Clone, Deserialize)]
pub struct OAuthMetadata {
    /// Where to send the user for authorization.
    #[serde(rename = "authorization_endpoint")]
    pub authorization: String,
    /// Where to exchange codes and refresh tokens.
    #[serde(rename = "token_endpoint")]
    pub token_exchange: String,
    /// Dynamic client registration endpoint (RFC 7591). Absent on servers that
    /// require a pre-registered `client_id`.
    #[serde(default, rename = "registration_endpoint")]
    pub registration: Option<String>,
}

/// Fetch OAuth metadata from `{origin}/.well-known/oauth-authorization-server`.
///
/// # Errors
///
/// Returns [`McpError::Transport`] on network failure or non-2xx status, and
/// [`McpError::Protocol`] if the response is not valid JSON metadata.
pub fn discover_metadata(server_url: &str) -> Result<OAuthMetadata, McpError> {
    let origin = extract_origin(server_url);
    let url = format!("{origin}/.well-known/oauth-authorization-server");
    let resp = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| McpError::Transport(format!("build HTTP client: {e}")))?
        .get(&url)
        .send()
        .map_err(|e| McpError::Transport(format!("metadata discovery: {e}")))?;
    if !resp.status().is_success() {
        return Err(McpError::Transport(format!(
            "metadata discovery returned {}",
            resp.status()
        )));
    }
    resp.json::<OAuthMetadata>()
        .map_err(|e| McpError::Protocol(format!("decode OAuth metadata: {e}")))
}

// ── Dynamic Client Registration (RFC 7591) ──────────────────────────────────

/// Result of dynamic client registration.
#[derive(Debug, Clone, Deserialize)]
pub struct ClientRegistration {
    /// Assigned client identifier.
    pub client_id: String,
    /// Client secret (rare for public clients using PKCE).
    #[serde(default)]
    pub client_secret: Option<String>,
}

/// Register a new client dynamically, if the server supports it.
///
/// # Errors
///
/// Returns errors on network failure or unsupported registration.
pub fn register_client(
    registration_endpoint: &str,
    redirect_uri: &str,
) -> Result<ClientRegistration, McpError> {
    let payload = serde_json::json!({
        "client_name": "Context Pilot",
        "redirect_uris": [redirect_uri],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    });
    let resp = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| McpError::Transport(format!("build HTTP client: {e}")))?
        .post(registration_endpoint)
        .header(CONTENT_TYPE, "application/json")
        .body(
            serde_json::to_string(&payload)
                .map_err(|e| McpError::Protocol(format!("encode registration: {e}")))?,
        )
        .send()
        .map_err(|e| McpError::Transport(format!("client registration: {e}")))?;
    if !resp.status().is_success() {
        let status = resp.status();
        return Err(McpError::Protocol(format!(
            "client registration returned {status}"
        )));
    }
    resp.json::<ClientRegistration>()
        .map_err(|e| McpError::Protocol(format!("decode registration: {e}")))
}

// ── Browser ─────────────────────────────────────────────────────────────────

/// Open a URL in the user's default browser.
///
/// Tries `xdg-open` (Linux), `open` (macOS), then gives up gracefully —
/// returning the URL in the error so the caller can display it.
///
/// # Errors
///
/// Returns [`McpError::Transport`] with the URL if no browser opener is found.
pub fn open_browser(url: &str) -> Result<(), McpError> {
    let openers = ["xdg-open", "open"];
    for cmd in &openers {
        let result = std::process::Command::new(cmd).arg(url).spawn();
        if result.is_ok() {
            return Ok(());
        }
    }
    Err(McpError::Transport(format!(
        "no browser opener found — open manually: {url}"
    )))
}

// ── Main Orchestration ──────────────────────────────────────────────────────

/// Run the full OAuth 2.1 + PKCE authorization flow for a remote MCP server.
///
/// Opens the user's default browser, waits for the OAuth callback on a
/// transient localhost listener (up to 120 s), exchanges the code, stores the
/// token, and returns the bearer access token.
///
/// # Errors
///
/// Propagates discovery, network, registration, PKCE, callback, or exchange
/// failures.
pub fn authorize(server_url: &str) -> Result<String, McpError> {
    let key = server_key(server_url);

    // 1. Already have a valid token?
    if let Some(token) = load_stored_token(&key) {
        return Ok(token);
    }

    // 2. Try refreshing an expired token.
    if let Ok(token) = try_refresh(&key) {
        return Ok(token);
    }

    // 3. Discover OAuth metadata.
    let metadata = discover_metadata(server_url)?;
    let pkce = generate_pkce()?;
    let state = generate_state()?;

    // 4. Start callback listener.
    let (listener, port) = start_listener()?;
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    // 5. Obtain a `client_id` (dynamic registration or stored).
    let client_id = resolve_client_id(&metadata, &redirect_uri, &key)?;

    let flow = AuthFlowParams {
        pkce: &pkce,
        state: &state,
        redirect_uri: &redirect_uri,
        client_id: &client_id,
    };

    // 6. Open browser.
    let auth_url = build_auth_url(&metadata, &flow);
    open_browser(&auth_url)?;

    // 7. Wait for callback.
    let callback = wait_for_redirect(&listener)?;
    if callback.state != state {
        return Err(McpError::Protocol("OAuth state mismatch (CSRF)".to_owned()));
    }

    // 8. Exchange code for tokens.
    let token_resp = exchange_code(&metadata, &callback.code, &flow)?;

    // 9. Store and return.
    store_token(&key, &token_resp, &client_id, &metadata.token_exchange)?;
    Ok(token_resp.access_token)
}

/// Attempt to refresh an expired token using stored credentials.
fn try_refresh(server_key: &str) -> Result<String, McpError> {
    let tokens = load_all();
    let stored = tokens
        .get(server_key)
        .ok_or_else(|| McpError::Protocol("no stored token".to_owned()))?;
    let refresh = stored
        .refresh_token
        .as_deref()
        .ok_or_else(|| McpError::Protocol("no refresh token".to_owned()))?;
    let client_id = stored
        .client_id
        .as_deref()
        .ok_or_else(|| McpError::Protocol("no stored client_id".to_owned()))?;
    let endpoint = stored
        .token_endpoint
        .as_deref()
        .ok_or_else(|| McpError::Protocol("no stored token_endpoint".to_owned()))?;

    let resp = refresh_access_token(endpoint, refresh, client_id)?;
    store_token(server_key, &resp, client_id, endpoint)?;
    Ok(resp.access_token)
}

/// Resolve a `client_id`: load from stored tokens, or dynamically register.
fn resolve_client_id(
    metadata: &OAuthMetadata,
    redirect_uri: &str,
    server_key: &str,
) -> Result<String, McpError> {
    // Check stored client_id first.
    if let Some(cid) = load_all()
        .get(server_key)
        .and_then(|stored| stored.client_id.clone())
    {
        return Ok(cid);
    }

    // Try dynamic registration.
    let endpoint = metadata.registration.as_deref().ok_or_else(|| {
        McpError::Protocol(
            "server has no registration_endpoint and no client_id stored — \
             add client_id to your MCP config"
                .to_owned(),
        )
    })?;
    let reg = register_client(endpoint, redirect_uri)?;
    Ok(reg.client_id)
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Extract the origin (`scheme://host[:port]`) from a URL.
#[must_use]
pub fn server_key(url: &str) -> String {
    extract_origin(url)
}

/// Extract `scheme://host[:port]` from a full URL.
fn extract_origin(url: &str) -> String {
    // Find the scheme separator, then the first slash after it.
    let Some(scheme_end) = url.find("://") else { return url.to_owned() };
    let after_scheme = scheme_end.saturating_add(3);
    let rest = url.get(after_scheme..).unwrap_or_default();
    rest.find('/').map_or_else(
        || url.to_owned(),
        |slash| {
            url.get(..after_scheme.saturating_add(slash))
                .unwrap_or(url)
                .to_owned()
        },
    )
}

/// Minimal percent-encoding for URL query values (covers the characters that
/// actually appear in OAuth parameters).
pub(crate) fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '+' => out.push_str("%2B"),
            '&' => out.push_str("%26"),
            '=' => out.push_str("%3D"),
            '%' => out.push_str("%25"),
            '/' => out.push_str("%2F"),
            ':' => out.push_str("%3A"),
            '?' => out.push_str("%3F"),
            '#' => out.push_str("%23"),
            _ => out.push(c),
        }
    }
    out
}

/// Percent-decode a URL query value (reverses `urlencoded` and standard
/// percent-encoding). `+` is decoded as a space (form-encoding convention).
pub(crate) fn urldecoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        match b {
            b'+' => out.push(' '),
            b'%' => {
                let hi = chars.next().unwrap_or(b'0');
                let lo = chars.next().unwrap_or(b'0');
                let val = hex_nibble(hi).wrapping_shl(4) | hex_nibble(lo);
                out.push(char::from(val));
            }
            _ => out.push(char::from(b)),
        }
    }
    out
}

/// Single hex digit → 0–15.
const fn hex_nibble(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b.wrapping_sub(b'0'),
        b'a'..=b'f' => b.wrapping_sub(b'a').wrapping_add(10),
        b'A'..=b'F' => b.wrapping_sub(b'A').wrapping_add(10),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_verifier_and_challenge_differ() {
        let Ok(pkce) = generate_pkce() else { return };
        assert_ne!(pkce.verifier, pkce.challenge);
        // Verifier is ~43 chars (32 bytes base64url).
        assert!(pkce.verifier.len() >= 40);
        assert!(pkce.verifier.len() <= 50);
    }

    #[test]
    fn state_is_nonempty() {
        let Ok(state) = generate_state() else { return };
        assert!(!state.is_empty());
    }

    #[test]
    fn extract_origin_strips_path() {
        assert_eq!(extract_origin("https://mcp.notion.com/mcp"), "https://mcp.notion.com");
        assert_eq!(extract_origin("http://localhost:8080/api/v1"), "http://localhost:8080");
        assert_eq!(extract_origin("https://example.com"), "https://example.com");
    }

    #[test]
    fn build_auth_url_includes_all_params() {
        let meta = OAuthMetadata {
            authorization: "https://example.com/authorize".to_owned(),
            token_exchange: String::new(),
            registration: None,
        };
        let Ok(pkce) = generate_pkce() else { return };
        let flow = AuthFlowParams {
            pkce: &pkce,
            state: "abc",
            redirect_uri: "http://localhost:9999/callback",
            client_id: "client123",
        };
        let url = build_auth_url(&meta, &flow);
        assert!(url.starts_with("https://example.com/authorize?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=client123"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=abc"));
    }

    #[test]
    fn urlencoded_encodes_special_chars() {
        assert_eq!(urlencoded("hello world"), "hello%20world");
        assert_eq!(urlencoded("a=b&c"), "a%3Db%26c");
        assert_eq!(urlencoded("https://x.com/path"), "https%3A%2F%2Fx.com%2Fpath");
    }

    #[test]
    fn urldecoded_reverses_encoding() {
        assert_eq!(urldecoded("hello%20world"), "hello world");
        assert_eq!(urldecoded("a%3Db%26c"), "a=b&c");
        assert_eq!(urldecoded("plain"), "plain");
        assert_eq!(urldecoded("hello+world"), "hello world");
    }

    #[test]
    fn server_key_extracts_origin() {
        assert_eq!(server_key("https://mcp.notion.com/mcp"), "https://mcp.notion.com");
    }
}
