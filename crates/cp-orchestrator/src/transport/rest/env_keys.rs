//! On-demand environment-key inspection (T399).
//!
//! Two endpoints:
//!
//! * `GET /api/env-keys` — list well-known env var names with exists/missing
//!   status. Accessible to any authenticated user (or anyone when auth is
//!   disabled).
//! * `GET /api/env-keys/{name}` — reveal a *masked* env-key value (first 4 +
//!   last 4 characters, middle redacted). When auth is active this is
//!   **admin-only** — regular users get `403`.
//!
//! The masked value is never loaded into the frontend until the user
//! explicitly clicks the reveal (eye) button, and the raw value never leaves
//! the orchestrator process.

use serde_json::json;

use super::HttpReply;
use crate::services::auth::types::{User, UserRole};

/// Well-known env var names the config panel can query.
///
/// Each entry is `(ENV_VAR_NAME, human label)`.  The list is intentionally
/// hard-coded to prevent arbitrary env-var enumeration.
const KNOWN_KEYS: &[(&str, &str)] = &[
    ("ANTHROPIC_API_KEY", "Anthropic"),
    ("GROQ_API_KEY", "Groq"),
    ("XAI_API_KEY", "Grok (xAI)"),
    ("DEEPSEEK_API_KEY", "DeepSeek"),
    ("MINIMAX_API_KEY", "MiniMax"),
    ("VOYAGE_API_KEY", "Voyage AI"),
    ("DATALAB_API_KEY", "Datalab"),
    ("BRAVE_API_KEY", "Brave Search"),
    ("FIRECRAWL_API_KEY", "Firecrawl"),
    ("GITHUB_TOKEN", "GitHub"),
];

/// `GET /api/env-keys` — list all well-known keys with their status.
///
/// Returns a JSON array of `{ env, label, exists }` objects.  No values are
/// included — callers learn *whether* a key is configured, not its content.
pub(crate) fn env_keys_list() -> HttpReply {
    let keys: Vec<serde_json::Value> = KNOWN_KEYS
        .iter()
        .map(|(env, label)| {
            let exists = std::env::var(env).is_ok();
            json!({ "env": *env, "label": *label, "exists": exists })
        })
        .collect();
    HttpReply::ok(&keys)
}

/// `GET /api/env-keys/{name}` — reveal a masked key value.
///
/// When auth is active (`auth_user` is `Some`), only system administrators
/// may call this.  When auth is disabled (`auth_user` is `None`), anyone can.
/// The raw value never leaves the process — only a masked form
/// (`sk-a••••••••••n3f7`) is returned.
///
/// Rejects unknown key names with `404` to prevent arbitrary env-var
/// enumeration.
pub(crate) fn env_key_reveal(
    name: &str,
    auth_user: Option<&User>,
) -> HttpReply {
    // Admin gate when auth is active.
    if auth_user.is_some_and(|u| u.role != UserRole::Admin) {
        return HttpReply::error(403, "admin required");
    }

    // Only allow revealing well-known keys.
    if !KNOWN_KEYS.iter().any(|(env, _)| *env == name) {
        return HttpReply::error(404, "unknown key name");
    }

    std::env::var(name).ok().map_or_else(
        || HttpReply::ok(&json!({ "env": name, "masked": null, "exists": false })),
        |val| {
            let masked = mask_key(&val);
            HttpReply::ok(&json!({ "env": name, "masked": masked, "exists": true }))
        },
    )
}

/// Mask a key value: keep the first 4 and last 4 characters, replace the
/// middle with dots.  Keys shorter than 9 chars are fully masked.
fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    let len = chars.len();
    if len <= 8 {
        return "••••••••".to_owned();
    }
    let prefix: String = chars.get(..4).map_or_else(String::new, |s| s.iter().collect());
    let start = len.wrapping_sub(4);
    let suffix: String = chars.get(start..).map_or_else(String::new, |s| s.iter().collect());
    format!("{prefix}••••••••••{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_key_short_is_fully_redacted() {
        assert_eq!(mask_key("abc"), "••••••••");
        assert_eq!(mask_key("12345678"), "••••••••");
    }

    #[test]
    fn mask_key_long_keeps_prefix_suffix() {
        let masked = mask_key("sk-ant-abcdef123456789xyz");
        assert!(masked.starts_with("sk-a"), "prefix mismatch: {masked}");
        assert!(masked.ends_with("9xyz"), "suffix mismatch: {masked}");
        assert!(masked.contains("••••••••••"), "middle not masked: {masked}");
    }

    #[test]
    fn env_keys_list_returns_array() {
        let reply = env_keys_list();
        assert_eq!(reply.status, 200);
        let arr: Vec<serde_json::Value> = serde_json::from_str(&reply.body).expect("valid json");
        assert_eq!(arr.len(), KNOWN_KEYS.len());
        for item in &arr {
            assert!(item.get("env").is_some());
            assert!(item.get("label").is_some());
            assert!(item.get("exists").is_some());
        }
    }

    #[test]
    fn reveal_rejects_unknown_key() {
        let reply = env_key_reveal("NOT_A_REAL_KEY", None);
        assert_eq!(reply.status, 404);
    }

    #[test]
    fn reveal_rejects_non_admin() {
        let user = User {
            id: "u1".to_owned(),
            email: "test@test.com".to_owned(),
            name: "Test".to_owned(),
            password_hash: String::new(),
            role: UserRole::User,
            created_at: 0,
            updated_at: 0,
        };
        let reply = env_key_reveal("ANTHROPIC_API_KEY", Some(&user));
        assert_eq!(reply.status, 403);
    }
}
