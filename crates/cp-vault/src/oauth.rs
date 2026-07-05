//! Claude OAuth token loading — consolidated from three duplicate implementations.
//!
//! Resolves the Claude Code OAuth access token from:
//! 1. macOS Keychain (service `"Claude Code-credentials"`)
//! 2. `~/.claude/.credentials.json` or `~/.claude/credentials.json`
//!
//! The token is checked for expiry before returning.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::types::SecretString;

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
/// Returns `None` if no valid, unexpired token is found.
pub(crate) fn load_claude_oauth_token() -> Option<SecretString> {
    if cfg!(target_os = "macos")
        && let Some(token) = load_from_keychain()
    {
        return Some(token);
    }
    load_from_file()
}

/// Read credentials JSON from the macOS Keychain via the `security` CLI.
///
/// The password stored under service `"Claude Code-credentials"` is the same
/// JSON blob as the on-disk credentials file.
fn load_from_keychain() -> Option<SecretString> {
    let output = Command::new("security")
        .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let content = String::from_utf8(output.stdout).ok()?;
    parse_credentials_json(content.trim())
}

/// Read credentials JSON from `~/.claude/.credentials.json` (or fallback path).
fn load_from_file() -> Option<SecretString> {
    let home = std::env::var("HOME").ok()?;
    let home_path = PathBuf::from(&home);

    let primary = home_path.join(".claude").join(".credentials.json");
    let fallback = home_path.join(".claude").join("credentials.json");

    let path = if primary.exists() { primary } else { fallback };
    let content = fs::read_to_string(&path).ok()?;
    parse_credentials_json(&content)
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
}
