//! Claude Code OAuth credential presence check.
//!
//! The cockpit surfaces the OAuth provider backends (`claudecode`,
//! `claudecodev2`) only when a usable credentials file is on disk — provisioned
//! out-of-band (see `deploy/ansible/claude-oauth.yml`), never via an in-box
//! OAuth flow. This mirrors the agent's reader: the box neither writes nor
//! refreshes the token, so an expired one simply reads as unavailable.

/// On-disk shape of `~/.claude/.credentials.json` — only the two fields the
/// agent's reader needs (`crates/.../llms/claude_code` mirrors this).
#[derive(serde::Deserialize)]
struct OAuthCredentialsFile {
    /// The OAuth credentials section.
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: OAuthCredentials,
}

/// OAuth credential fields within the credentials file.
#[derive(serde::Deserialize)]
struct OAuthCredentials {
    /// Bearer access token.
    #[serde(rename = "accessToken")]
    access_token: String,
    /// Token expiry, milliseconds since the UNIX epoch.
    #[serde(rename = "expiresAt")]
    expires_at: u64,
}

/// Is a usable Claude Code OAuth credential present on disk? Reads
/// `$HOME/.claude/.credentials.json` (or the `credentials.json` fallback), gated
/// on a non-empty `accessToken` and a future `expiresAt`. The box never
/// refreshes — an expired token reads as unavailable (rotate via the
/// provisioning playbook).
pub(super) fn claude_oauth_available() -> bool {
    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    let dir = std::path::PathBuf::from(home).join(".claude");
    let dotted = dir.join(".credentials.json");
    let path = if dotted.exists() { dotted } else { dir.join("credentials.json") };

    let Ok(bytes) = std::fs::read(&path) else {
        return false;
    };
    let now_ms =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(u128::MAX, |d| d.as_millis());
    oauth_creds_usable(&bytes, now_ms)
}

/// Pure verdict for [`claude_oauth_available`]: do `bytes` parse to a credentials
/// file with a non-empty `accessToken` that has not expired at `now_ms`?
fn oauth_creds_usable(bytes: &[u8], now_ms: u128) -> bool {
    let Ok(creds) = serde_json::from_slice::<OAuthCredentialsFile>(bytes) else {
        return false;
    };
    !creds.claude_ai_oauth.access_token.is_empty() && u128::from(creds.claude_ai_oauth.expires_at) > now_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds(token: &str, expires_at: u64) -> String {
        format!(r#"{{"claudeAiOauth":{{"accessToken":"{token}","expiresAt":{expires_at}}}}}"#)
    }

    #[test]
    fn oauth_usable_when_token_present_and_unexpired() {
        assert!(oauth_creds_usable(creds("sk-ant-oat01-x", 2_000).as_bytes(), 1_000));
    }

    #[test]
    fn oauth_unusable_when_expired() {
        assert!(!oauth_creds_usable(creds("sk-ant-oat01-x", 1_000).as_bytes(), 2_000));
    }

    #[test]
    fn oauth_unusable_when_token_empty_or_garbage() {
        assert!(!oauth_creds_usable(creds("", 2_000).as_bytes(), 1_000));
        assert!(!oauth_creds_usable(b"not json", 1_000));
        assert!(!oauth_creds_usable(br#"{"unexpected":1}"#, 1_000));
    }
}
