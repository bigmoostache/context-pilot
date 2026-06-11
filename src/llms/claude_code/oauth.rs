//! Claude Code OAuth helpers: token refresh and usage API.
//!
//! Shared between `claude_code` and `claude_code_v2` providers.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use cp_base::cast::Safe as _;

use crate::llms::error::LlmError;

/// OAuth client ID for Claude Code (from captured traffic)
const OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

/// Token refresh endpoint
const TOKEN_REFRESH_ENDPOINT: &str = "https://console.anthropic.com/v1/oauth/token";

/// Usage API endpoint
const USAGE_ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";

/// OAuth credentials structure (matches both Keychain and file format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct OAuthCredentials {
    /// Bearer access token
    pub access_token: String,
    /// Refresh token for getting new access tokens
    pub refresh_token: String,
    /// Token expiry timestamp in milliseconds since UNIX epoch
    pub expires_at: u64,
}

/// On-disk credentials file structure
#[derive(Deserialize)]
struct CredentialsFile {
    /// OAuth credentials section
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: OAuthCredentialsInFile,
}

/// OAuth credential fields within the credentials file.
#[derive(Deserialize)]
struct OAuthCredentialsInFile {
    /// Bearer access token
    #[serde(rename = "accessToken")]
    access_token: String,
    /// Refresh token for getting new access tokens
    #[serde(rename = "refreshToken")]
    refresh_token: String,
    /// Token expiry timestamp in milliseconds since UNIX epoch
    #[serde(rename = "expiresAt")]
    expires_at: u64,
}

/// Token refresh response
#[derive(Deserialize)]
pub(crate) struct TokenRefreshResponse {
    /// New access token
    pub access_token: String,
    /// New refresh token (replaces the old one)
    pub refresh_token: String,
    /// New expiry timestamp in milliseconds since UNIX epoch
    pub expires_at: u64,
}

/// Check if token is expired (or will expire within 60 seconds)
pub(crate) fn is_token_expired(expires_at_ms: u64) -> bool {
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().to_u64());
    let buffer_ms = 60_000; // 60 second safety buffer
    expires_at_ms < now_ms.saturating_add(buffer_ms)
}

/// Load OAuth credentials from macOS Keychain or credentials file
pub(crate) fn load_oauth_credentials() -> Option<OAuthCredentials> {
    // Try macOS Keychain first
    if cfg!(target_os = "macos")
        && let Some(creds) = load_from_keychain()
    {
        return Some(creds);
    }
    // Fall back to credentials file
    load_from_file()
}

/// Load from macOS Keychain
fn load_from_keychain() -> Option<OAuthCredentials> {
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

/// Load from ~/.claude/.credentials.json
fn load_from_file() -> Option<OAuthCredentials> {
    let home = env::var("HOME").ok()?;
    let home_path = PathBuf::from(&home);
    let creds_path = home_path.join(".claude").join(".credentials.json");
    let path = if creds_path.exists() {
        creds_path
    } else {
        home_path.join(".claude").join("credentials.json")
    };
    let content = fs::read_to_string(&path).ok()?;
    parse_credentials_json(&content)
}

/// Parse credentials JSON
fn parse_credentials_json(content: &str) -> Option<OAuthCredentials> {
    let creds: CredentialsFile = serde_json::from_str(content).ok()?;
    Some(OAuthCredentials {
        access_token: creds.claude_ai_oauth.access_token,
        refresh_token: creds.claude_ai_oauth.refresh_token,
        expires_at: creds.claude_ai_oauth.expires_at,
    })
}

/// Save OAuth credentials back to storage
pub(crate) fn save_oauth_credentials(creds: &OAuthCredentials) -> Result<(), Box<dyn std::error::Error>> {
    // Determine storage location
    if cfg!(target_os = "macos") {
        // Try to update Keychain first
        if let Err(e) = save_to_keychain(creds) {
            // Log warning but fall back to file (non-fatal)
            drop(std::io::Write::write_fmt(
                &mut std::io::stderr(),
                format_args!("Warning: failed to update Keychain, falling back to file: {e}\n"),
            ));
            save_to_file(creds)?;
        }
    } else {
        save_to_file(creds)?;
    }
    Ok(())
}

/// Save to macOS Keychain
fn save_to_keychain(creds: &OAuthCredentials) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::json!({
        "claudeAiOauth": {
            "accessToken": creds.access_token,
            "refreshToken": creds.refresh_token,
            "expiresAt": creds.expires_at
        }
    });
    let json_str = serde_json::to_string(&json)?;
    
    // Update existing keychain entry
    let status = Command::new("security")
        .args([
            "add-generic-password",
            "-U",  // Update if exists
            "-s", "Claude Code-credentials",
            "-a", "claude-code",
            "-w", &json_str
        ])
        .status()?;
    
    if !status.success() {
        return Err("Failed to update Keychain".into());
    }
    Ok(())
}

/// Save to ~/.claude/.credentials.json
fn save_to_file(creds: &OAuthCredentials) -> Result<(), Box<dyn std::error::Error>> {
    let home = env::var("HOME")?;
    let home_path = PathBuf::from(&home);
    let claude_dir = home_path.join(".claude");
    fs::create_dir_all(&claude_dir)?;
    
    let json = serde_json::json!({
        "claudeAiOauth": {
            "accessToken": creds.access_token,
            "refreshToken": creds.refresh_token,
            "expiresAt": creds.expires_at
        }
    });
    
    let path = claude_dir.join(".credentials.json");
    fs::write(&path, serde_json::to_string_pretty(&json)?)?;
    Ok(())
}

/// Refresh OAuth token if expired
pub(crate) fn refresh_token_if_needed(creds: &mut OAuthCredentials) -> Result<(), LlmError> {
    if !is_token_expired(creds.expires_at) {
        return Ok(()); // Still valid
    }
    
    let client = reqwest::blocking::Client::new();
    let response = client
        .post(TOKEN_REFRESH_ENDPOINT)
        .header("Content-Type", "application/json")
        .header("User-Agent", "anthropic")
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": creds.refresh_token,
            "client_id": OAUTH_CLIENT_ID
        }))
        .send()
        .map_err(|e| LlmError::Network(format!("Token refresh failed: {e}")))?;
    
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().unwrap_or_default();
        return Err(LlmError::Auth(format!(
            "Token refresh failed {status}: {body}"
        )));
    }
    
    let tokens: TokenRefreshResponse = response
        .json()
        .map_err(|e| LlmError::Network(format!("Failed to parse refresh response: {e}")))?;
    
    // Update credentials
    creds.access_token = tokens.access_token;
    creds.refresh_token = tokens.refresh_token;
    creds.expires_at = tokens.expires_at;
    
    // Save back to storage
    if let Err(e) = save_oauth_credentials(creds) {
        // Log warning but don't fail (non-fatal)
        drop(std::io::Write::write_fmt(
            &mut std::io::stderr(),
            format_args!("Warning: refreshed token but failed to save: {e}\n"),
        ));
    }
    
    Ok(())
}

/// Fetch current OAuth usage from the usage API.
///
/// Returns 5-hour and 7-day utilization percentages. Requires valid OAuth token.
/// Rate limit: safe at 180-second intervals with correct User-Agent.
pub(crate) fn fetch_usage(access_token: &str) -> Result<cp_base::config::llm_types::UsageResponse, LlmError> {
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(USAGE_ENDPOINT)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "claude-code/2.1.173")
        .header("Content-Type", "application/json")
        .send()
        .map_err(|e| LlmError::Network(format!("Usage fetch failed: {e}")))?;
    
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().unwrap_or_default();
        return Err(LlmError::Api {
            status,
            body: format!("Usage API: {body}"),
        });
    }
    
    response
        .json()
        .map_err(|e| LlmError::Network(format!("Failed to parse usage response: {e}")))
}

