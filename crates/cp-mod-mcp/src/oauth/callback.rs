//! PKCE generation, authorization URL building, and localhost callback listener.
//!
//! Handles the interactive parts of the OAuth flow: PKCE challenge/verifier,
//! constructing the browser-facing authorization URL, and listening on a
//! transient localhost port for the OAuth provider's redirect.

use std::collections::HashMap;
use std::io::Read as _;
use std::net::TcpListener;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest as _, Sha256};

use crate::errors::McpError;

/// Timeout for the localhost callback listener (seconds).
const CALLBACK_TIMEOUT_SECS: u64 = 120;

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
/// on [`build_auth_url`] and [`exchange_code`](super::tokens::exchange_code)).
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
pub fn build_auth_url(metadata: &super::OAuthMetadata, params: &AuthFlowParams<'_>) -> String {
    let sep = if metadata.authorization.contains('?') { '&' } else { '?' };
    format!(
        "{}{sep}response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state={}",
        metadata.authorization,
        super::urlencoded(params.client_id),
        super::urlencoded(params.redirect_uri),
        super::urlencoded(&params.pkce.challenge),
        super::urlencoded(params.state),
    )
}

// ── Localhost Callback Listener ─────────────────────────────────────────────

/// The authorization code and state returned by the OAuth redirect.
#[derive(Debug, Clone)]
pub struct AuthRedirect {
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
pub fn wait_for_redirect(listener: &TcpListener) -> Result<AuthRedirect, McpError> {
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
    let code = super::urldecoded(code);
    let state = params
        .get("state")
        .ok_or_else(|| McpError::Protocol("callback missing 'state' param".to_owned()))?;
    let state = super::urldecoded(state);

    Ok(AuthRedirect { code, state })
}
