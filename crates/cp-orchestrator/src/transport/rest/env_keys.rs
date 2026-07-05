//! On-demand environment-key inspection and editing (T399, T404).
//!
//! Delegates to the [`cp_vault`] credential vault for key resolution and
//! persistence.  Three endpoints:
//!
//! * `GET /api/env-keys` — list all known keys with exists/missing status.
//! * `GET /api/env-keys/{name}` — reveal a key value (admin-only when auth
//!   active).
//! * `PUT /api/env-keys/{name}` — update a key value (admin-only).  Persists
//!   to `~/.context-pilot/.env` and sets an in-memory override so the change
//!   is immediately visible.

use serde_json::json;

use super::HttpReply;
use crate::services::auth::types::{User, UserRole};

/// `GET /api/vault/snapshot` — bulk-fetch all set key values.
///
/// Returns a JSON object `{ "canonical_name": "value", ... }` containing every
/// key that currently resolves to a value.  Designed for [`BridgeVault`] cache
/// warm-up.  Keys without a value are omitted (not `null`).
///
/// Admin-only when auth is enabled.
pub(crate) fn vault_snapshot(auth_user: Option<&User>) -> HttpReply {
    if auth_user.is_some_and(|u| u.role != UserRole::Admin) {
        return HttpReply::error(403, "admin required");
    }

    let snapshot: serde_json::Map<String, serde_json::Value> = cp_vault::registry::ALL_KEYS
        .iter()
        .filter(|k| !k.env_var.is_empty())
        .filter_map(|k| {
            cp_vault::vault()
                .get(k.canonical)
                .map(|s| (k.canonical.to_owned(), serde_json::Value::String(s.expose().to_owned())))
        })
        .collect();
    HttpReply::ok(&snapshot)
}

/// `GET /api/env-keys` — list all known keys with their status.
///
/// Returns a JSON array of `{ env, label, exists }` objects.  Keys without an
/// env var name (OAuth-only credentials) are excluded.
pub(crate) fn env_keys_list() -> HttpReply {
    let keys: Vec<serde_json::Value> = cp_vault::registry::ALL_KEYS
        .iter()
        .filter(|k| !k.env_var.is_empty())
        .map(|k| {
            let exists = cp_vault::vault().get(k.canonical).is_some();
            json!({ "env": k.env_var, "label": k.display, "exists": exists })
        })
        .collect();
    HttpReply::ok(&keys)
}

/// `GET /api/env-keys/{name}` — reveal a key value (admin-only).
///
/// Accepts both env var names (`ANTHROPIC_API_KEY`) and canonical names
/// (`anthropic`).  Rejects unknown names with `404` to prevent arbitrary
/// environment enumeration.
pub(crate) fn env_key_reveal(name: &str, auth_user: Option<&User>) -> HttpReply {
    if auth_user.is_some_and(|u| u.role != UserRole::Admin) {
        return HttpReply::error(403, "admin required");
    }

    if cp_vault::registry::resolve_definition(name).is_none() {
        return HttpReply::error(404, "unknown key name");
    }

    cp_vault::vault().get(name).map_or_else(
        || HttpReply::ok(&json!({ "env": name, "value": null, "masked": null, "exists": false })),
        |secret| {
            let val = secret.expose();
            let masked = mask_key(val);
            HttpReply::ok(&json!({ "env": name, "value": val, "masked": masked, "exists": true }))
        },
    )
}

/// `PUT /api/env-keys/{name}` — update a key value (admin-only).
///
/// Accepts a JSON body `{ "value": "..." }`.  Delegates to
/// [`cp_vault::vault().set()`](cp_vault::types::Vault::set) which persists to
/// `~/.context-pilot/.env` and stores an in-memory override for immediate
/// visibility.
pub(crate) fn env_key_update(name: &str, auth_user: Option<&User>, body: &str) -> HttpReply {
    if auth_user.is_some_and(|u| u.role != UserRole::Admin) {
        return HttpReply::error(403, "admin required");
    }

    if cp_vault::registry::resolve_definition(name).is_none() {
        return HttpReply::error(404, "unknown key name");
    }

    let parsed: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return HttpReply::error(400, "invalid JSON"),
    };
    let value = match parsed.get("value").and_then(serde_json::Value::as_str) {
        Some(v) => v,
        None => return HttpReply::error(400, "missing string field 'value'"),
    };

    if let Err(e) = cp_vault::vault().set(name, value) {
        return HttpReply::error(502, &e.to_string());
    }

    HttpReply::ok(&json!({
        "env": name,
        "value": value,
        "masked": mask_key(value),
        "exists": true,
        "persisted": true,
    }))
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

    #[test]
    fn update_rejects_non_admin() {
        let user = User {
            id: "u1".to_owned(),
            email: "test@test.com".to_owned(),
            name: "Test".to_owned(),
            password_hash: String::new(),
            role: UserRole::User,
            created_at: 0,
            updated_at: 0,
        };
        let reply = env_key_update("ANTHROPIC_API_KEY", Some(&user), r#"{"value":"sk-new"}"#);
        assert_eq!(reply.status, 403);
    }

    #[test]
    fn update_rejects_unknown_key() {
        let reply = env_key_update("NOT_REAL", None, r#"{"value":"val"}"#);
        assert_eq!(reply.status, 404);
    }
}
