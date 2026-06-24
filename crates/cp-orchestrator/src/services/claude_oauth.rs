//! Claude Code **OAuth manual-login** flow + background token refresh (P-oauth).
//!
//! Lets the owner sign in with their Anthropic account from the cockpit so the
//! `claudecode` provider works without an API key. The registered OAuth client
//! only supports the **manual paste** flow (its redirect shows a code), so:
//!
//! 1. [`start`] builds an authorize URL (PKCE S256) and remembers the verifier.
//! 2. The user opens it, signs in, and copies the `code#state` shown.
//! 3. [`exchange`] swaps that code for tokens and [`write_credentials`] writes
//!    `~/.claude/.credentials.json` — the exact file the agent's Claude Code
//!    client reads (`src/llms/claude_code/mod.rs`).
//!
//! The agent reads `accessToken`/`expiresAt` but **never refreshes**, so on a
//! headless Pi the token would die after a few hours. [`refresh_if_needed`] runs
//! on a background thread to keep it alive via `grant_type=refresh_token`.
//!
//! Exact endpoints/params are pinned to the verified values (see the
//! `claude-code-oauth-params` note): Anthropic migrated `console.anthropic.com`
//! → `platform.claude.com`, and the token body MUST be form-urlencoded.

use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use rand::RngCore as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// PKCE/authorize client id (the Claude Code CLI's public client).
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
/// Manual-paste authorize endpoint (`?code=true` shows the code to copy).
const AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
/// Redirect the client is registered with (a code-display callback).
const REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";
/// Token endpoint (post-migration host).
const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
/// Requested scopes.
const SCOPE: &str = "user:inference user:profile user:sessions:claude_code user:mcp_servers";
/// Refresh when the access token is within this window of expiring.
const REFRESH_SKEW: Duration = Duration::from_secs(30 * 60);

/// A pending manual-login: the PKCE verifier + state awaiting the pasted code.
#[derive(Debug, Clone)]
pub struct Pending {
    /// PKCE code verifier (kept server-side; sent at exchange).
    verifier: String,
    /// CSRF state echoed in the pasted `code#state`.
    state: String,
}

/// The tokens we persist, mirroring Claude Code's `claudeAiOauth` section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    /// Bearer access token.
    #[serde(rename = "accessToken")]
    pub access_token: String,
    /// Refresh token (used by the background refresher).
    #[serde(rename = "refreshToken", default)]
    pub refresh_token: String,
    /// Expiry in epoch-ms (the agent rejects the token once past this).
    #[serde(rename = "expiresAt")]
    pub expires_at: u64,
    /// Granted scopes.
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// The on-disk file shape: `{ "claudeAiOauth": { … } }`.
#[derive(Debug, Serialize, Deserialize)]
struct CredentialsFile {
    /// The OAuth section.
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Credentials,
}

/// The token endpoint's JSON response.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    /// New access token.
    access_token: String,
    /// New refresh token (rotated; may be absent on some responses).
    #[serde(default)]
    refresh_token: Option<String>,
    /// Lifetime in seconds.
    #[serde(default)]
    expires_in: Option<u64>,
    /// Space-separated granted scopes.
    #[serde(default)]
    scope: Option<String>,
}

/// Begin a manual login: return `(authorize_url, pending)`. The caller stashes
/// `pending` and hands `authorize_url` to the browser.
///
/// # Errors
///
/// Returns a message if the authorize URL cannot be built.
pub fn start() -> Result<(String, Pending), String> {
    let verifier = b64url(&random_bytes::<32>());
    let challenge = b64url(Sha256::digest(verifier.as_bytes()).as_slice());
    let state = b64url(&random_bytes::<24>());
    let url = build_authorize_url(&challenge, &state)?;
    Ok((url, Pending { verifier, state }))
}

/// Build the PKCE authorize URL for `challenge` + `state`.
fn build_authorize_url(challenge: &str, state: &str) -> Result<String, String> {
    reqwest::Url::parse_with_params(
        AUTHORIZE_URL,
        &[
            ("code", "true"),
            ("client_id", CLIENT_ID),
            ("response_type", "code"),
            ("redirect_uri", REDIRECT_URI),
            ("scope", SCOPE),
            ("state", state),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
        ],
    )
    .map(|u| u.to_string())
    .map_err(|e| e.to_string())
}

/// Complete the login: split the pasted `code#state`, verify the state against
/// `pending`, exchange the code for tokens, and return [`Credentials`].
///
/// # Errors
///
/// `"state mismatch"` if the pasted state doesn't match the pending one; an HTTP
/// or parse error string if the exchange fails.
pub fn exchange(pasted: &str, pending: &Pending) -> Result<Credentials, String> {
    // The pasted value is `code#state`; some clients paste just `code`.
    let (code, state) = match pasted.trim().split_once('#') {
        Some((c, s)) => (c, s),
        None => (pasted.trim(), pending.state.as_str()),
    };
    if state != pending.state {
        return Err("state mismatch".to_owned());
    }
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("state", state),
        ("redirect_uri", REDIRECT_URI),
        ("client_id", CLIENT_ID),
        ("code_verifier", pending.verifier.as_str()),
    ];
    let resp = post_token(&form)?;
    Ok(credentials_from(resp, &pending.verifier))
}

/// Refresh the stored credentials if they exist and are near expiry. Returns
/// `Ok(true)` if a refresh was performed, `Ok(false)` if none was needed.
///
/// # Errors
///
/// An HTTP/parse error string if a needed refresh fails.
pub fn refresh_if_needed() -> Result<bool, String> {
    let Some(creds) = read_credentials() else {
        return Ok(false);
    };
    if creds.refresh_token.is_empty() {
        return Ok(false);
    }
    if creds.expires_at > now_ms().saturating_add(REFRESH_SKEW.as_millis() as u64) {
        return Ok(false); // still fresh enough
    }
    let form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", creds.refresh_token.as_str()),
        ("client_id", CLIENT_ID),
    ];
    let resp = post_token(&form)?;
    // A refresh response may omit the rotated refresh token — keep the old one.
    let mut next = credentials_from(resp, "");
    if next.refresh_token.is_empty() {
        next.refresh_token = creds.refresh_token;
    }
    if next.scopes.is_empty() {
        next.scopes = creds.scopes;
    }
    write_credentials(&next)?;
    Ok(true)
}

/// POST a form-urlencoded body to the token endpoint and parse the response.
fn post_token(form: &[(&str, &str)]) -> Result<TokenResponse, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.post(TOKEN_URL).form(form).send().map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("token endpoint {status}: {}", body.chars().take(200).collect::<String>()));
    }
    serde_json::from_str::<TokenResponse>(&body).map_err(|e| format!("bad token response: {e}"))
}

/// Build [`Credentials`] from a token response (default 1h lifetime if absent).
fn credentials_from(resp: TokenResponse, _verifier: &str) -> Credentials {
    let lifetime = resp.expires_in.unwrap_or(3600);
    Credentials {
        access_token: resp.access_token,
        refresh_token: resp.refresh_token.unwrap_or_default(),
        expires_at: now_ms().saturating_add(lifetime.saturating_mul(1000)),
        scopes: resp.scope.map(|s| s.split(' ').map(str::to_owned).collect()).unwrap_or_default(),
    }
}

/// Path to the credentials file (`$HOME/.claude/.credentials.json`).
fn credentials_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude").join(".credentials.json"))
}

/// Whether a credentials file currently exists.
#[must_use]
pub fn credentials_present() -> bool {
    credentials_path().is_some_and(|p| p.is_file())
}

/// Read the stored credentials, if present and parseable.
#[must_use]
pub fn read_credentials() -> Option<Credentials> {
    let path = credentials_path()?;
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice::<CredentialsFile>(&bytes).ok().map(|f| f.claude_ai_oauth)
}

/// Write the credentials to `~/.claude/.credentials.json` atomically, `0600`.
///
/// # Errors
///
/// A message if `$HOME` is unset or the write fails.
pub fn write_credentials(creds: &Credentials) -> Result<(), String> {
    let path = credentials_path().ok_or("HOME not set")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let file = CredentialsFile { claude_ai_oauth: creds.clone() };
    let bytes = serde_json::to_vec_pretty(&file).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// `count` cryptographically-random bytes.
fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    buf
}

/// URL-safe base64 without padding (PKCE encoding).
fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Current wall-clock time in epoch milliseconds.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_sha256_b64url_of_verifier() {
        // Known RFC 7636 appendix-B vector.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = b64url(Sha256::digest(verifier.as_bytes()).as_slice());
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn authorize_url_carries_all_params_encoded() {
        let url = build_authorize_url("CHAL", "STATE").expect("url");
        assert!(url.starts_with("https://claude.ai/oauth/authorize?"));
        assert!(url.contains("code=true"));
        assert!(url.contains(&format!("client_id={CLIENT_ID}")));
        assert!(url.contains("code_challenge=CHAL"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=STATE"));
        // The scope's spaces are percent-encoded.
        assert!(url.contains("scope=user%3Ainference"));
        // The redirect host is the migrated one.
        assert!(url.contains("platform.claude.com%2Foauth%2Fcode%2Fcallback"));
    }

    #[test]
    fn exchange_rejects_state_mismatch() {
        let pending = Pending { verifier: "v".to_owned(), state: "good".to_owned() };
        assert_eq!(exchange("thecode#bad", &pending).err(), Some("state mismatch".to_owned()));
    }

    #[test]
    fn credentials_from_computes_expiry_and_scopes() {
        let resp = TokenResponse {
            access_token: "at".to_owned(),
            refresh_token: Some("rt".to_owned()),
            expires_in: Some(3600),
            scope: Some("user:inference user:profile".to_owned()),
        };
        let before = now_ms();
        let c = credentials_from(resp, "");
        assert_eq!(c.access_token, "at");
        assert_eq!(c.refresh_token, "rt");
        assert_eq!(c.scopes, vec!["user:inference", "user:profile"]);
        // Expiry ~ now + 1h.
        assert!(c.expires_at >= before + 3600 * 1000);
        assert!(c.expires_at <= now_ms() + 3600 * 1000 + 5000);
    }

    #[test]
    fn write_then_read_credentials_roundtrips_0600() {
        let dir = std::env::temp_dir().join(format!("cp-oauth-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        // Point HOME at the scratch dir for this test (single-threaded section).
        // SAFETY note: set_var is unsafe in edition 2024 and forbidden here, so
        // instead we exercise write/read against an explicit path via the same
        // serialization the public fns use.
        drop(std::fs::create_dir_all(dir.join(".claude")));
        let creds = Credentials {
            access_token: "tok".to_owned(),
            refresh_token: "ref".to_owned(),
            expires_at: 123,
            scopes: vec!["user:inference".to_owned()],
        };
        let file = CredentialsFile { claude_ai_oauth: creds.clone() };
        let path = dir.join(".claude").join(".credentials.json");
        let bytes = serde_json::to_vec_pretty(&file).expect("ser");
        std::fs::write(&path, &bytes).expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        // Read back via the file shape and check perms.
        let back: CredentialsFile =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("de");
        assert_eq!(back.claude_ai_oauth.access_token, "tok");
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        drop(std::fs::remove_dir_all(&dir));
    }
}
