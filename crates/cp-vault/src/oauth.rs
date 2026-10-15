//! Claude OAuth token loading — consolidated from three duplicate implementations.
//!
//! Resolves the Claude Code OAuth access token from:
//! 1. macOS Keychain (service `"Claude Code-credentials"`)
//! 2. `~/.claude/.credentials.json` or `~/.claude/credentials.json`
//!
//! The token is checked for expiry before returning. Token lookups are
//! memoized for [`TOKEN_TTL`]: each Keychain read spawns the `security` CLI,
//! and `Vault::list`/`health` resolve every key.

use std::fs;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use zeroize::Zeroizing;

use crate::types::SecretString;

/// How long a token lookup (hit or miss) is reused before re-reading.
/// Short, so a token rotated by the orchestrator's refresh flow is picked up
/// within seconds.
const TOKEN_TTL: Duration = Duration::from_secs(30);

/// When a token lookup ran and what it found.
type CachedLookup = (Instant, Option<SecretString>);

/// Last token lookup.
static TOKEN_CACHE: Mutex<Option<CachedLookup>> = Mutex::new(None);

/// On-disk credentials file structure for Claude Code OAuth.
#[derive(Deserialize)]
struct CredentialsFile {
    /// OAuth credentials section.
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: OAuthCredentials,
}

/// OAuth credential fields within the credentials file.
#[derive(Deserialize)]
struct OAuthCredentials {
    /// Bearer access token.
    #[serde(rename = "accessToken")]
    access_token: String,
    /// Token expiry timestamp in milliseconds since UNIX epoch.
    #[serde(rename = "expiresAt")]
    expires_at: u64,
}

/// Load the Claude OAuth token from Keychain (macOS) or credential file.
///
/// Returns `None` if no valid, unexpired token is found. Memoized for
/// [`TOKEN_TTL`]; a cached token is re-checked so it is never served expired.
///
/// `pub(crate)` by design: this is the *mechanism* behind
/// [`AuthMechanism::KeychainThenFile`](cp_env::specs::secrets::AuthMechanism) and is
/// called only by the vault's own [`resolve`](crate::local) cascade. Every
/// external consumer (agents, orchestrator) MUST go through
/// [`vault().get("claude_oauth")`](crate::types::Vault::get) instead, so the
/// in-memory overrides layer is always honored. The restricted visibility makes
/// bypassing the vault a compile error rather than a convention.
#[must_use]
pub(crate) fn load_claude_oauth_token() -> Option<SecretString> {
    let now = Instant::now();
    if let Ok(guard) = TOKEN_CACHE.lock()
        && let Some(entry) = guard.as_ref()
        && now.duration_since(entry.0) < TOKEN_TTL
    {
        return entry.1.clone();
    }
    let fresh = lookup_token();
    if let Ok(mut guard) = TOKEN_CACHE.lock() {
        *guard = Some((now, fresh.clone()));
    }
    fresh
}

/// Uncached lookup: Keychain first (macOS), then the credentials file.
fn lookup_token() -> Option<SecretString> {
    if cfg!(target_os = "macos")
        && let Some(token) = keychain_text().and_then(|t| parse_credentials_json(t.trim()))
    {
        return Some(token);
    }
    let text = file_text()?;
    parse_credentials_json(&text)
}

/// Credentials JSON stored under Keychain service `"Claude Code-credentials"`
/// (same blob as the on-disk file), via the `security` CLI.
fn keychain_text() -> Option<Zeroizing<String>> {
    let output = Command::new("security")
        .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok().map(Zeroizing::new)
}

/// Credentials JSON from `~/.claude/.credentials.json` (or `credentials.json`).
fn file_text() -> Option<Zeroizing<String>> {
    let claude_dir = cp_env::env().core.home.join(".claude");
    let primary = claude_dir.join(".credentials.json");
    let path = if primary.exists() { primary } else { claude_dir.join("credentials.json") };
    fs::read_to_string(&path).ok().map(Zeroizing::new)
}

/// Parse a credentials JSON blob and return the access token if not expired.
fn parse_credentials_json(content: &str) -> Option<SecretString> {
    let creds: CredentialsFile = serde_json::from_str(content).ok()?;

    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_millis();
    if now_ms > u128::from(creds.claude_ai_oauth.expires_at) {
        return None;
    }

    Some(SecretString::new(creds.claude_ai_oauth.access_token))
}

/// The `claudeAiOauth` object of a credentials JSON blob.
fn raw_section(content: &str) -> Option<serde_json::Value> {
    let val: serde_json::Value = serde_json::from_str(content.trim()).ok()?;
    val.get("claudeAiOauth").cloned()
}

/// Load the full Claude OAuth credentials JSON from Keychain or file.
///
/// Returns the raw `claudeAiOauth` object (with `accessToken`, `refreshToken`,
/// `expiresAt`, etc.) without expiry filtering.  Used by the orchestrator for
/// token-status inspection and refresh flows, so it is never memoized: a
/// refresh must see the token it just wrote.
#[must_use]
pub fn load_claude_oauth_raw() -> Option<serde_json::Value> {
    if cfg!(target_os = "macos")
        && let Some(val) = keychain_text().and_then(|t| raw_section(&t))
    {
        return Some(val);
    }
    let text = file_text()?;
    raw_section(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_credentials() {
        let future_ms: u64 = 9_999_999_999_999; // year ~2286
        let json = format!(r#"{{"claudeAiOauth":{{"accessToken":"sk-test-123","expiresAt":{future_ms}}}}}"#);
        let result = parse_credentials_json(&json);
        assert!(result.is_some());
        assert_eq!(result.map(|s| s.expose().to_owned()), Some("sk-test-123".to_owned()));
    }

    #[test]
    fn parse_expired_credentials_returns_none() {
        let json = r#"{"claudeAiOauth":{"accessToken":"sk-old","expiresAt":1000}}"#;
        let result = parse_credentials_json(json);
        assert!(result.is_none());
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        assert!(parse_credentials_json("not json").is_none());
    }

    #[test]
    fn raw_section_extracts_object() {
        let raw = raw_section(r#" {"claudeAiOauth":{"refreshToken":"r"}} "#).unwrap();
        assert_eq!(raw.get("refreshToken").and_then(|v| v.as_str()), Some("r"));
        assert!(raw_section("{}").is_none());
    }
}
