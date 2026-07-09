//! Claude Code OAuth credential presence check.
//!
//! The cockpit surfaces the OAuth provider backends (`claudecode`,
//! `claudecodev2`) only when a usable OAuth credential is resolvable. The check
//! goes through the SAME source the agent runtime uses —
//! [`cp_vault::oauth::load_claude_oauth_raw`], whose cascade is macOS Keychain
//! first (service `"Claude Code-credentials"`), then the credential file
//! `~/.claude/.credentials.json` (the Linux deploy box, provisioned out-of-band
//! via `deploy/ansible/claude-oauth.yml`).
//!
//! Reading the file directly here was a bug (T522): on a macOS dev box the real
//! Claude Code CLI stores the token in the Keychain, never that file, so the
//! file-only check reported "unavailable" and both OAuth providers were dropped
//! from `GET /api/providers` — even though the agent's own LLM calls, which read
//! through the vault, worked fine. Routing through the vault unifies the two.
//!
//! The box never refreshes — an expired token reads as unavailable (rotate via
//! the provisioning playbook / a fresh CLI login).

/// Is a usable Claude Code OAuth credential resolvable? Delegates the source
/// resolution to [`cp_vault::oauth::load_claude_oauth_raw`] (Keychain → file),
/// gated on a non-empty `accessToken` and a future `expiresAt`.
pub(super) fn claude_oauth_available() -> bool {
    let Some(creds) = cp_vault::oauth::load_claude_oauth_raw() else {
        return false;
    };
    let now_ms =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(u128::MAX, |d| d.as_millis());
    oauth_creds_usable(&creds, now_ms)
}

/// Pure verdict for [`claude_oauth_available`]: does the resolved `claudeAiOauth`
/// object carry a non-empty `accessToken` that has not expired at `now_ms`?
///
/// `creds` is the inner `claudeAiOauth` object (already unwrapped by the vault
/// loader), so its fields are read directly: `accessToken` (string) and
/// `expiresAt` (epoch-ms number). A missing field defaults to unusable.
fn oauth_creds_usable(creds: &serde_json::Value, now_ms: u128) -> bool {
    let token = creds.get("accessToken").and_then(serde_json::Value::as_str).unwrap_or_default();
    let expires_at = creds.get("expiresAt").and_then(serde_json::Value::as_u64).unwrap_or_default();
    !token.is_empty() && u128::from(expires_at) > now_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `claudeAiOauth` inner object (what the vault loader returns).
    fn creds(token: &str, expires_at: u64) -> serde_json::Value {
        serde_json::json!({ "accessToken": token, "expiresAt": expires_at })
    }

    #[test]
    fn oauth_usable_when_token_present_and_unexpired() {
        assert!(oauth_creds_usable(&creds("sk-ant-oat01-x", 2_000), 1_000));
    }

    #[test]
    fn oauth_unusable_when_expired() {
        assert!(!oauth_creds_usable(&creds("sk-ant-oat01-x", 1_000), 2_000));
    }

    #[test]
    fn oauth_unusable_when_token_empty_or_fields_missing() {
        assert!(!oauth_creds_usable(&creds("", 2_000), 1_000));
        assert!(!oauth_creds_usable(&serde_json::json!({ "unexpected": 1 }), 1_000));
        assert!(!oauth_creds_usable(&serde_json::Value::Null, 1_000));
    }
}
