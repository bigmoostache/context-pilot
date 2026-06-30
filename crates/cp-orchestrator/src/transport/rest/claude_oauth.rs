//! Claude Code OAuth helpers — usage proxy, token status, and PKCE login flow.
//!
//! The login flow implements RFC 7636 (PKCE) with S256 code-challenge against
//! Anthropic's OAuth endpoints. The orchestrator generates the PKCE pair,
//! builds the authorize URL, and the user completes authorization in their
//! browser, then pastes the resulting code back into the frontend dialog.

use std::net::TcpListener;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{Backend, HttpReply};

// ── Constants ────────────────────────────────────────────────────────

const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const SCOPES: &str = "user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";
/// PKCE sessions expire after 5 minutes.
const PKCE_TTL_SECS: u64 = 300;

// ── PKCE session (stored in Backend) ─────────────────────────────────

/// In-flight PKCE session — lives between `/start` and `/complete`.
#[derive(Debug)]
pub(crate) struct PkceSession {
    code_verifier: String,
    /// `http://localhost:{port}/callback` — must match between authorize and
    /// token-exchange requests (RFC 8252 §7.3 loopback redirect).
    redirect_uri: String,
    created_at: Instant,
}

// ── Usage proxy ──────────────────────────────────────────────────────

/// `GET /api/claude-usage` — proxy live Claude Code OAuth usage limits.
///
/// Reads the user's OAuth token from the macOS Keychain (or
/// `~/.claude/.credentials.json`) and fetches the session/weekly rate-limit
/// percentages from Anthropic's `/api/oauth/usage` endpoint.
pub(crate) fn claude_usage() -> HttpReply {
    let Some(token) = read_access_token() else {
        return HttpReply::error(404, "Claude Code OAuth token not found");
    };
    let client = reqwest::blocking::Client::new();
    let resp = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .header("User-Agent", "claude-code/2.1.196")
        .header("anthropic-beta", "oauth-2025-04-20")
        .timeout(Duration::from_secs(10))
        .send();
    match resp {
        Ok(r) => match r.json::<serde_json::Value>() {
            Ok(val) => HttpReply { status: 200, body: val.to_string() },
            Err(e) => HttpReply::error(502, &format!("invalid usage response: {e}")),
        },
        Err(e) => HttpReply::error(502, &format!("usage fetch failed: {e}")),
    }
}

// ── Token status ─────────────────────────────────────────────────────

/// `GET /api/claude-login/status` — check whether a valid OAuth token exists.
pub(crate) fn token_status() -> HttpReply {
    let creds = read_credentials_json();
    match creds {
        Some(oauth) => {
            let expires_at = oauth.get("expiresAt").and_then(|v| v.as_i64()).unwrap_or(0);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let valid = expires_at > now_ms && oauth.get("accessToken").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty());
            HttpReply::ok(&TokenStatusResponse {
                valid,
                expires_at: if expires_at > 0 { Some(expires_at) } else { None },
                subscription_type: oauth.get("subscriptionType").and_then(|v| v.as_str()).map(str::to_owned),
                rate_limit_tier: oauth.get("rateLimitTier").and_then(|v| v.as_str()).map(str::to_owned),
            })
        }
        None => HttpReply::ok(&TokenStatusResponse { valid: false, expires_at: None, subscription_type: None, rate_limit_tier: None }),
    }
}

// ── Login start ──────────────────────────────────────────────────────

/// `POST /api/claude-login/start` — generate PKCE pair and return the
/// authorize URL for the user to open in their browser.
pub(crate) fn login_start(state: &Mutex<Backend>) -> HttpReply {
    // RFC 8252 §7.3: bind a random loopback port for the redirect URI.
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(e) => return HttpReply::error(500, &format!("could not bind loopback port: {e}")),
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(18923);
    let redirect_uri = format!("http://localhost:{port}/callback");

    // Generate code_verifier: 32 random bytes → base64url (43 chars).
    let mut verifier_bytes = [0u8; 32];
    if read_random(&mut verifier_bytes).is_err() {
        return HttpReply::error(500, "could not generate random bytes");
    }
    let code_verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);

    // code_challenge = base64url(SHA-256(code_verifier))
    let challenge_hash = Sha256::digest(code_verifier.as_bytes());
    let code_challenge = URL_SAFE_NO_PAD.encode(challenge_hash);

    // State parameter (CSRF protection).
    let mut state_bytes = [0u8; 32];
    if read_random(&mut state_bytes).is_err() {
        return HttpReply::error(500, "could not generate random bytes");
    }
    let state_param = URL_SAFE_NO_PAD.encode(state_bytes);

    let url = format!(
        "{AUTHORIZE_URL}?code=true&client_id={CLIENT_ID}&response_type=code&redirect_uri={}&scope={}&code_challenge={code_challenge}&code_challenge_method=S256&state={state_param}",
        urlencoded(&redirect_uri),
        urlencoded(SCOPES),
    );

    // Store PKCE session (login_complete fallback for manual code-paste).
    if let Ok(mut b) = state.lock() {
        b.pkce_session = Some(PkceSession {
            code_verifier: code_verifier.clone(),
            redirect_uri: redirect_uri.clone(),
            created_at: Instant::now(),
        });
    }

    // Listener is dropped — the redirect will hit a dead port, but the
    // authorization code is displayed on the Anthropic page (code=true)
    // and also visible in the browser's URL bar. The user copies it and
    // pastes it into the frontend dialog.
    drop(listener);

    HttpReply::ok(&LoginStartResponse { url })
}

// ── Login complete ───────────────────────────────────────────────────

/// `POST /api/claude-login/complete` — exchange the authorization code for
/// tokens and store them in the macOS Keychain / credentials file.
///
/// This is the manual fallback — the user pastes the code from the browser.
/// Normally the [`spawn_callback_listener`] auto-completes the exchange.
pub(crate) fn login_complete(state: &Mutex<Backend>, body_bytes: &[u8]) -> HttpReply {
    let Ok(req) = serde_json::from_slice::<LoginCompleteRequest>(body_bytes) else {
        return HttpReply::error(400, "expected {\"code\":\"...\"}");
    };
    let code = req.code.trim();
    if code.is_empty() {
        return HttpReply::error(400, "code is required");
    }

    // Retrieve and consume the PKCE session.
    let session = state.lock().ok().and_then(|mut b| b.pkce_session.take());
    let Some(session) = session else {
        return HttpReply::error(400, "no pending login — call /start first");
    };
    if session.created_at.elapsed().as_secs() > PKCE_TTL_SECS {
        return HttpReply::error(400, "login session expired — please start again");
    }

    match exchange_and_store(code, &session.code_verifier, &session.redirect_uri) {
        Ok(expires_at) => HttpReply::ok(&LoginCompleteResponse { status: "ok".to_owned(), expires_at: Some(expires_at) }),
        Err(e) => HttpReply::error(502, &e),
    }
}

// ── Token exchange (shared) ──────────────────────────────────────────

/// Exchange an authorization code for tokens and persist credentials.
///
/// Returns `expires_at` (ms since epoch) on success.
/// Used by both [`spawn_callback_listener`] (auto) and [`login_complete`] (manual).
fn exchange_and_store(code: &str, code_verifier: &str, redirect_uri: &str) -> Result<i64, String> {
    let body = serde_json::json!({
        "grant_type": "authorization_code",
        "code": code,
        "code_verifier": code_verifier,
        "client_id": CLIENT_ID,
        "redirect_uri": redirect_uri,
    });
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .timeout(Duration::from_secs(15))
        .send()
        .map_err(|e| format!("token exchange failed: {e}"))?;

    let status = resp.status();
    let text = resp.text().map_err(|e| format!("reading token response: {e}"))?;
    let val: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("invalid token JSON: {e} — body: {}", &text[..text.len().min(500)]))?;
    if !status.is_success() {
        let msg = val.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str())
            .unwrap_or_else(|| val.get("error").and_then(|e| e.as_str()).unwrap_or("token exchange failed"));
        return Err(format!("{msg} (HTTP {status})"));
    }

    let access_token = val.get("access_token").and_then(|v| v.as_str()).unwrap_or("");
    let refresh_token = val.get("refresh_token").and_then(|v| v.as_str()).unwrap_or("");
    let expires_in = val.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(0);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let expires_at = now_ms + expires_in * 1000;

    let creds = serde_json::json!({
        "claudeAiOauth": {
            "accessToken": access_token,
            "refreshToken": refresh_token,
            "expiresAt": expires_at,
            "scopes": SCOPES.split(' ').collect::<Vec<_>>(),
        }
    });

    store_credentials(&creds)?;
    Ok(expires_at)
}

// ── Credential I/O ───────────────────────────────────────────────────

/// Read the `claudeAiOauth` object from Keychain or credentials file.
fn read_credentials_json() -> Option<serde_json::Value> {
    // macOS Keychain (preferred).
    if let Ok(out) = std::process::Command::new("security")
        .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
        .output()
    {
        if out.status.success() {
            if let Ok(raw) = std::str::from_utf8(&out.stdout) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(raw.trim()) {
                    return val.get("claudeAiOauth").cloned();
                }
            }
        }
    }
    // Fallback: credentials file.
    let home = std::env::var("HOME").ok()?;
    let path = std::path::Path::new(&home).join(".claude/.credentials.json");
    let data = std::fs::read_to_string(path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&data).ok()?;
    val.get("claudeAiOauth").cloned()
}

/// Read the OAuth access token (convenience wrapper over [`read_credentials_json`]).
fn read_access_token() -> Option<String> {
    read_credentials_json()?.get("accessToken")?.as_str().map(str::to_owned)
}

/// Store credentials in macOS Keychain (primary) and `~/.claude/.credentials.json` (fallback).
fn store_credentials(creds: &serde_json::Value) -> Result<(), String> {
    let json = serde_json::to_string(creds).map_err(|e| e.to_string())?;

    // Try macOS Keychain first.
    let keychain_ok = std::process::Command::new("security")
        .args(["add-generic-password", "-U", "-s", "Claude Code-credentials", "-a", "Claude Code-credentials", "-w", &json])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    // Always write the credentials file as fallback.
    let home = std::env::var("HOME").map_err(|_| "HOME not set")?;
    let claude_dir = std::path::Path::new(&home).join(".claude");
    let _mkdir = std::fs::create_dir_all(&claude_dir);
    let creds_path = claude_dir.join(".credentials.json");
    std::fs::write(&creds_path, &json).map_err(|e| format!("write credentials: {e}"))?;

    if !keychain_ok {
        eprintln!("warning: could not store credentials in macOS Keychain — saved to {}", creds_path.display());
    }
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Minimal percent-encoding for URL query parameters.
fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => {
                out.push('%');
                out.push(char::from(b"0123456789ABCDEF"[(b >> 4) as usize]));
                out.push(char::from(b"0123456789ABCDEF"[(b & 0x0F) as usize]));
            }
        }
    }
    out
}

/// Read random bytes from `/dev/urandom`.
fn read_random(buf: &mut [u8]) -> Result<(), std::io::Error> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")?.read_exact(buf)
}

// ── Response types ───────────────────────────────────────────────────

#[derive(Serialize)]
struct TokenStatusResponse {
    valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subscription_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rate_limit_tier: Option<String>,
}

#[derive(Serialize)]
struct LoginStartResponse {
    url: String,
}

#[derive(Deserialize)]
struct LoginCompleteRequest {
    code: String,
}

#[derive(Serialize)]
struct LoginCompleteResponse {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<i64>,
}
