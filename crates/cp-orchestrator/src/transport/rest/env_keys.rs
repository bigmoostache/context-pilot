//! On-demand environment-key inspection and editing (T399, T404).
//!
//! Three endpoints:
//!
//! * `GET /api/env-keys` — list well-known env var names with exists/missing
//!   status. Accessible to any authenticated user (or anyone when auth is
//!   disabled).
//! * `GET /api/env-keys/{name}` — reveal an env-key value.  When auth is
//!   active this is **admin-only** — regular users get `403`.  Returns the
//!   *full* value so admins can copy-paste it, plus a masked rendition for
//!   display.
//! * `PUT /api/env-keys/{name}` — update an env-key value (admin-only).
//!   Writes to `~/.context-pilot/.env` for persistence across restarts and
//!   stores an in-memory override so the change is immediately visible.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::PathBuf;

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

/// Resolve an env var's value, checking runtime overrides first.
fn resolve_env(name: &str, overrides: &HashMap<String, String>) -> Option<String> {
    overrides.get(name).cloned().or_else(|| std::env::var(name).ok())
}

/// `GET /api/env-keys` — list all well-known keys with their status.
///
/// Returns a JSON array of `{ env, label, exists }` objects.  No values are
/// included — callers learn *whether* a key is configured, not its content.
pub(crate) fn env_keys_list(overrides: &HashMap<String, String>) -> HttpReply {
    let keys: Vec<serde_json::Value> = KNOWN_KEYS
        .iter()
        .map(|(env, label)| {
            let exists = resolve_env(env, overrides).is_some();
            json!({ "env": *env, "label": *label, "exists": exists })
        })
        .collect();
    HttpReply::ok(&keys)
}

/// `GET /api/env-keys/{name}` — reveal a key value (admin-only).
///
/// Returns both the full `value` and a `masked` rendition.  When auth is
/// disabled (`auth_user` is `None`) anyone may call this.  The raw value is
/// included so admins can copy-paste; the masked form is provided for display.
///
/// Rejects unknown key names with `404` to prevent arbitrary env-var
/// enumeration.
pub(crate) fn env_key_reveal(name: &str, auth_user: Option<&User>, overrides: &HashMap<String, String>) -> HttpReply {
    // Admin gate when auth is active.
    if auth_user.is_some_and(|u| u.role != UserRole::Admin) {
        return HttpReply::error(403, "admin required");
    }

    // Only allow revealing well-known keys.
    if !KNOWN_KEYS.iter().any(|(env, _)| *env == name) {
        return HttpReply::error(404, "unknown key name");
    }

    resolve_env(name, overrides).map_or_else(
        || HttpReply::ok(&json!({ "env": name, "value": null, "masked": null, "exists": false })),
        |val| {
            let masked = mask_key(&val);
            HttpReply::ok(&json!({ "env": name, "value": val, "masked": masked, "exists": true }))
        },
    )
}

/// `PUT /api/env-keys/{name}` — update an env-key value (admin-only).
///
/// Accepts a JSON body `{ "value": "..." }`.  Writes the new value into the
/// global `~/.context-pilot/.env` for persistence across restarts and stores
/// an in-memory override so the change is immediately visible in the UI.
///
/// Agents pick up the change on their next launch (they load the global
/// `.env` at boot via `dotenvy`).
pub(crate) fn env_key_update(
    name: &str,
    auth_user: Option<&User>,
    body: &str,
    overrides: &mut HashMap<String, String>,
) -> HttpReply {
    // Admin gate when auth is active.
    if auth_user.is_some_and(|u| u.role != UserRole::Admin) {
        return HttpReply::error(403, "admin required");
    }

    // Only allow updating well-known keys.
    if !KNOWN_KEYS.iter().any(|(env, _)| *env == name) {
        return HttpReply::error(404, "unknown key name");
    }

    // Parse body.
    let parsed: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return HttpReply::error(400, "invalid JSON"),
    };
    let value = match parsed.get("value").and_then(serde_json::Value::as_str) {
        Some(v) => v,
        None => return HttpReply::error(400, "missing string field 'value'"),
    };

    // Persist to ~/.context-pilot/.env.
    if let Err(msg) = persist_to_global_env(name, value) {
        return HttpReply::error(502, &msg);
    }

    // Store in-memory override for immediate visibility.
    drop(overrides.insert(name.to_owned(), value.to_owned()));

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

/// Write or update a `NAME=value` entry in `~/.context-pilot/.env`.
///
/// Creates the file (and parent directory) if it does not exist.  Preserves
/// comments and other entries.  Values containing spaces or special characters
/// are double-quoted.
fn persist_to_global_env(name: &str, value: &str) -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|_| "HOME not set".to_owned())?;
    let env_dir = PathBuf::from(&home).join(".context-pilot");
    let env_path = env_dir.join(".env");

    // Ensure directory exists.
    std::fs::create_dir_all(&env_dir).map_err(|e| format!("cannot create {}: {e}", env_dir.display()))?;

    // Read existing content (ok if missing).
    let existing = std::fs::read_to_string(&env_path).unwrap_or_default();

    let prefix = format!("{name}=");
    let mut found = false;
    let mut lines: Vec<String> = existing
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with(&prefix) && !trimmed.starts_with('#') {
                found = true;
                format_env_line(name, value)
            } else {
                line.to_owned()
            }
        })
        .collect();

    if !found {
        // Ensure trailing newline before appending.
        if lines.last().is_some_and(|l| !l.is_empty()) {
            lines.push(String::new());
        }
        lines.push(format_env_line(name, value));
    }

    let mut content = lines.join("\n");
    if !content.ends_with('\n') {
        content.push('\n');
    }

    let mut file = std::fs::File::create(&env_path).map_err(|e| format!("cannot write {}: {e}", env_path.display()))?;
    file.write_all(content.as_bytes()).map_err(|e| format!("write failed: {e}"))?;

    Ok(())
}

/// Format an env line, quoting the value if it contains shell-sensitive chars.
fn format_env_line(name: &str, value: &str) -> String {
    if value.contains(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '#' || c == '$') {
        // Double-quote and escape inner double-quotes.
        let escaped = value.replace('"', "\\\"");
        format!("{name}=\"{escaped}\"")
    } else {
        format!("{name}={value}")
    }
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
        let overrides = HashMap::new();
        let reply = env_keys_list(&overrides);
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
        let overrides = HashMap::new();
        let reply = env_key_reveal("NOT_A_REAL_KEY", None, &overrides);
        assert_eq!(reply.status, 404);
    }

    #[test]
    fn reveal_rejects_non_admin() {
        let overrides = HashMap::new();
        let user = User {
            id: "u1".to_owned(),
            email: "test@test.com".to_owned(),
            name: "Test".to_owned(),
            password_hash: String::new(),
            role: UserRole::User,
            created_at: 0,
            updated_at: 0,
        };
        let reply = env_key_reveal("ANTHROPIC_API_KEY", Some(&user), &overrides);
        assert_eq!(reply.status, 403);
    }

    #[test]
    fn format_env_line_plain_value() {
        assert_eq!(format_env_line("KEY", "abc123"), "KEY=abc123");
    }

    #[test]
    fn format_env_line_quotes_spaces() {
        assert_eq!(format_env_line("KEY", "has space"), "KEY=\"has space\"");
    }

    #[test]
    fn format_env_line_escapes_inner_quotes() {
        assert_eq!(format_env_line("KEY", "has\"quote"), "KEY=\"has\\\"quote\"");
    }

    #[test]
    fn resolve_env_prefers_override() {
        let mut overrides = HashMap::new();
        drop(overrides.insert("TEST_KEY".to_owned(), "override_val".to_owned()));
        assert_eq!(resolve_env("TEST_KEY", &overrides), Some("override_val".to_owned()));
    }

    #[test]
    fn update_rejects_non_admin() {
        let mut overrides = HashMap::new();
        let user = User {
            id: "u1".to_owned(),
            email: "test@test.com".to_owned(),
            name: "Test".to_owned(),
            password_hash: String::new(),
            role: UserRole::User,
            created_at: 0,
            updated_at: 0,
        };
        let reply = env_key_update("ANTHROPIC_API_KEY", Some(&user), r#"{"value":"sk-new"}"#, &mut overrides);
        assert_eq!(reply.status, 403);
    }

    #[test]
    fn update_rejects_unknown_key() {
        let mut overrides = HashMap::new();
        let reply = env_key_update("NOT_REAL", None, r#"{"value":"val"}"#, &mut overrides);
        assert_eq!(reply.status, 404);
    }
}
