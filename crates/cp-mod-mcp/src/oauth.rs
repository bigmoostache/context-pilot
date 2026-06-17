//! OAuth 2.1 + PKCE authorization flow for remote MCP servers.
//!
//! Implements the MCP spec's recommended auth path: discover OAuth metadata at
//! `{origin}/.well-known/oauth-authorization-server`, run an authorization-code
//! flow with PKCE, and persist tokens in `~/.context-pilot/mcp-tokens.json`.
//!
//! The [`authorize`] entry point opens the user's browser and waits on a
//! transient localhost callback listener. It is deliberately **blocking** — call
//! it from a setup CLI or a background thread, never from `init_state`.

use std::collections::HashMap;
use std::io::Read as _;
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reqwest::blocking::Client;
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::errors::McpError;

/// Timeout for the localhost callback listener (seconds).
const CALLBACK_TIMEOUT_SECS: u64 = 120;

/// Token file relative to the global config directory.
const TOKENS_FILE: &str = "mcp-tokens.json";

/// Safety margin (seconds) subtracted from `expires_at` to avoid clock-skew
/// races. A stored token is considered expired 60 s before its real deadline.
const EXPIRY_MARGIN_SECS: u64 = 60;

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

// ── PKCE ────────────────────────────────────────────────────────────────────

/// PKCE code-verifier / code-challenge pair.
#[derive(Debug, Clone)]
pub struct Pkce {
    /// High-entropy verifier sent during token exchange.
    pub verifier: String,
    /// `S256(verifier)` sent during the authorization request.
    pub challenge: String,
}

/// Generate a fresh PKCE pair: 32 random bytes → base64url verifier, SHA-256 →
/// base64url challenge.
///
/// # Errors
///
/// Returns [`McpError::Transport`] if `/dev/urandom` cannot be read.
pub fn generate_pkce() -> Result<Pkce, McpError> {
    let random = random_bytes(32)?;
    let verifier = URL_SAFE_NO_PAD.encode(&random);
    let digest = Sha256::digest(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(digest);
    Ok(Pkce { verifier, challenge })
}

/// Generate a random state parameter for CSRF protection (16 bytes, base64url).
///
/// # Errors
///
/// Returns [`McpError::Transport`] if `/dev/urandom` cannot be read.
pub fn generate_state() -> Result<String, McpError> {
    let random = random_bytes(16)?;
    Ok(URL_SAFE_NO_PAD.encode(&random))
}

/// Read `n` bytes from `/dev/urandom`.
fn random_bytes(n: usize) -> Result<Vec<u8>, McpError> {
    let mut buf = vec![0u8; n];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf).map(|()| buf))
        .map_err(|e| McpError::Transport(format!("read /dev/urandom: {e}")))
}

// ── Authorization URL ───────────────────────────────────────────────────────

/// Bundled parameters for the authorization code flow (reduces argument count
/// on [`build_auth_url`] and [`exchange_code`]).
#[derive(Debug)]
pub struct AuthFlowParams<'flow> {
    /// PKCE verifier/challenge pair.
    pub pkce: &'flow Pkce,
    /// CSRF state nonce (for the authorization URL).
    pub state: &'flow str,
    /// Localhost callback URI.
    pub redirect_uri: &'flow str,
    /// Client identifier.
    pub client_id: &'flow str,
}

/// Build the browser-facing authorization URL with PKCE and CSRF state.
#[must_use]
pub fn build_auth_url(metadata: &OAuthMetadata, params: &AuthFlowParams<'_>) -> String {
    let sep = if metadata.authorization.contains('?') { '&' } else { '?' };
    format!(
        "{}{sep}response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state={}",
        metadata.authorization,
        urlencoded(params.client_id),
        urlencoded(params.redirect_uri),
        urlencoded(&params.pkce.challenge),
        urlencoded(params.state),
    )
}

/// Minimal percent-encoding for URL query values (covers the characters that
/// actually appear in OAuth parameters).
fn urlencoded(s: &str) -> String {
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
fn urldecoded(s: &str) -> String {
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

// ── Localhost Callback Listener ─────────────────────────────────────────────

/// The authorization code and state returned by the OAuth callback.
#[derive(Debug, Clone)]
pub struct CallbackResult {
    /// Authorization code to exchange for tokens.
    pub code: String,
    /// CSRF state to verify against the original.
    pub state: String,
}

/// Start a localhost TCP listener on a random port and return the listener +
/// the port for the redirect URI.
///
/// # Errors
///
/// Returns [`McpError::Transport`] if binding fails.
pub fn start_listener() -> Result<(TcpListener, u16), McpError> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| McpError::Transport(format!("bind callback listener: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| McpError::Transport(format!("listener local addr: {e}")))?
        .port();
    listener
        .set_nonblocking(false)
        .map_err(|e| McpError::Transport(format!("set blocking: {e}")))?;
    Ok((listener, port))
}

/// Block until the OAuth provider redirects to our listener, or
/// [`CALLBACK_TIMEOUT_SECS`] elapses.
///
/// # Errors
///
/// Returns [`McpError::Timeout`] if no request arrives in time, or
/// [`McpError::Protocol`] if the request cannot be parsed.
pub fn wait_for_callback(listener: &TcpListener) -> Result<CallbackResult, McpError> {
    listener
        .set_nonblocking(true)
        .map_err(|e| McpError::Transport(format!("set nonblocking: {e}")))?;

    let deadline = Instant::now().checked_add(Duration::from_secs(CALLBACK_TIMEOUT_SECS));
    let (mut stream, _addr) = loop {
        match listener.accept() {
            Ok(pair) => break pair,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if deadline.is_some_and(|d| Instant::now() >= d) {
                    return Err(McpError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(e) => {
                return Err(McpError::Transport(format!("accept callback: {e}")));
            }
        }
    };

    // Switch the accepted stream to blocking for the read.
    stream
        .set_nonblocking(false)
        .map_err(|e| McpError::Transport(format!("set stream blocking: {e}")))?;

    // Read the HTTP request (small — just the GET line + headers).
    let mut buf = vec![0u8; 4096];
    let n = stream
        .read(&mut buf)
        .map_err(|e| McpError::Transport(format!("read callback: {e}")))?;
    buf.truncate(n);
    let request = String::from_utf8_lossy(&buf);

    // Send a minimal response before parsing (browser gets feedback immediately).
    let body = "<html><body><h2>Authorization complete</h2><p>You can close this tab.</p></body></html>";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _write = std::io::Write::write_all(&mut stream, response.as_bytes());

    // Parse GET /callback?code=...&state=... HTTP/1.1
    let query = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|path| path.split_once('?'))
        .map(|(_, qs)| qs)
        .ok_or_else(|| McpError::Protocol("callback missing query string".to_owned()))?;

    let params: HashMap<&str, &str> = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .collect();

    let code = params
        .get("code")
        .ok_or_else(|| McpError::Protocol("callback missing 'code' param".to_owned()))?;
    let code = urldecoded(code);
    let state = params
        .get("state")
        .ok_or_else(|| McpError::Protocol("callback missing 'state' param".to_owned()))?;
    let state = urldecoded(state);

    Ok(CallbackResult { code, state })
}

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
    metadata: &OAuthMetadata,
    code: &str,
    params: &AuthFlowParams<'_>,
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
        .timeout(Duration::from_secs(15))
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
pub fn tokens_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".context-pilot").join(TOKENS_FILE))
}

/// Load all stored tokens, or an empty map if the file is absent / corrupt.
#[must_use]
pub fn load_all_tokens() -> HashMap<String, StoredToken> {
    let Some(path) = tokens_path() else { return HashMap::new() };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Load a stored token for a server key, returning `None` if absent or expired.
#[must_use]
pub fn load_stored_token(server_key: &str) -> Option<String> {
    let tokens = load_all_tokens();
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
    let mut tokens = load_all_tokens();
    let _prev = tokens.insert(server_key.to_owned(), entry);
    let Some(path) = tokens_path() else {
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
    let callback = wait_for_callback(&listener)?;
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
    let tokens = load_all_tokens();
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
    if let Some(cid) = load_all_tokens()
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

/// Current wall-clock time as Unix seconds.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
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
