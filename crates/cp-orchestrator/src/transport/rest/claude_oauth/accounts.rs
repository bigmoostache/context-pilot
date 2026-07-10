//! Multi-account Claude OAuth token vault — store, list, switch, delete.
//!
//! Persists inactive tokens in `~/.context-pilot/claude-accounts.json` keyed
//! by account email. The *active* token lives in the macOS Keychain /
//! `~/.claude/.credentials.json` (managed by [`super::store_credentials`]).
//! Switching swaps the active credential with a stored one — zero restart.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::super::HttpReply;

const ACCOUNTS_FILE: &str = "claude-accounts.json";

// ── Stored file format ───────────────────────────────────────────────

/// On-disk shape of `~/.context-pilot/claude-accounts.json`.
#[derive(Debug, Serialize, Deserialize, Default)]
struct AccountsFile {
    /// Email → full credential blob (same shape as `claudeAiOauth`).
    accounts: BTreeMap<String, serde_json::Value>,
}

fn accounts_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    std::path::Path::new(&home).join(".context-pilot").join(ACCOUNTS_FILE)
}

fn read_accounts() -> AccountsFile {
    std::fs::read_to_string(accounts_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn write_accounts(store: &AccountsFile) -> Result<(), String> {
    let json = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    let path = accounts_path();
    if let Some(parent) = path.parent() {
        let _mkdir = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, json).map_err(|e| format!("write {}: {e}", path.display()))
}

// ── Helpers ──────────────────────────────────────────────────────────

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

// ── Response types ───────────────────────────────────────────────────

#[derive(Serialize)]
struct AccountSummary {
    email: String,
    expires_at: Option<i64>,
    valid: bool,
}

#[derive(Serialize)]
struct AccountsListResponse {
    accounts: Vec<AccountSummary>,
}

#[derive(Deserialize)]
struct SwitchRequest {
    email: String,
}

// ── Handlers ─────────────────────────────────────────────────────────

/// `GET /api/claude-accounts` — list stored (inactive) accounts.
pub(crate) fn list_accounts() -> HttpReply {
    let store = read_accounts();
    let now = now_ms();
    let accounts: Vec<AccountSummary> = store
        .accounts
        .iter()
        .map(|(email, creds)| {
            let expires_at = creds.get("expiresAt").and_then(|v| v.as_i64());
            let token = creds.get("accessToken").and_then(|v| v.as_str()).unwrap_or("");
            let valid = expires_at.is_some_and(|e| e > now) && !token.is_empty();
            AccountSummary { email: email.clone(), expires_at, valid }
        })
        .collect();
    HttpReply::ok(&AccountsListResponse { accounts })
}

/// `POST /api/claude-accounts/store` — save the current active token
/// under its account email. Does NOT remove it from the active slot.
pub(crate) fn store_account() -> HttpReply {
    let Some(active) = super::read_credentials_json() else {
        return HttpReply::error(404, "no active Claude OAuth token found");
    };
    let token = active.get("accessToken").and_then(|v| v.as_str()).unwrap_or("");
    if token.is_empty() {
        return HttpReply::error(400, "active token has no accessToken");
    }
    let Some(email) = super::fetch_account_email(token) else {
        return HttpReply::error(502, "could not detect account email from active token");
    };

    let mut store = read_accounts();
    let _prev = store.accounts.insert(email.clone(), active);
    if let Err(e) = write_accounts(&store) {
        return HttpReply::error(500, &e);
    }
    HttpReply::ok(&serde_json::json!({ "ok": true, "email": email }))
}

/// `POST /api/claude-accounts/switch` — swap: store the current active
/// token, then load the selected stored token into the active slot.
pub(crate) fn switch_account(body_bytes: &[u8]) -> HttpReply {
    let Ok(req) = serde_json::from_slice::<SwitchRequest>(body_bytes) else {
        return HttpReply::error(400, "expected {\"email\":\"...\"}");
    };
    let email = req.email.trim();
    if email.is_empty() {
        return HttpReply::error(400, "email is required");
    }

    let mut store = read_accounts();
    let Some(target_creds) = store.accounts.remove(email) else {
        return HttpReply::error(404, &format!("no stored account for {email}"));
    };

    // If the access token expired, attempt a refresh before activating.
    let target_creds = maybe_refresh(target_creds);

    // Save current active into the store (best-effort: if no active token
    // exists we still proceed with the switch).
    if let Some(current) = super::read_credentials_json() {
        let current_token = current.get("accessToken").and_then(|v| v.as_str()).unwrap_or("");
        if !current_token.is_empty() {
            if let Some(current_email) = super::fetch_account_email(current_token) {
                let _prev = store.accounts.insert(current_email, current);
            }
        }
    }

    // Write updated store (with old active added, target removed).
    if let Err(e) = write_accounts(&store) {
        return HttpReply::error(500, &e);
    }

    // Activate the target credentials.
    let wrapped = serde_json::json!({ "claudeAiOauth": target_creds });
    if let Err(e) = super::store_credentials(&wrapped) {
        return HttpReply::error(500, &format!("failed to activate credentials: {e}"));
    }

    HttpReply::ok(&serde_json::json!({ "ok": true, "email": email }))
}

/// `DELETE /api/claude-accounts/{email}` — remove a stored account.
pub(crate) fn delete_account(email: &str) -> HttpReply {
    let mut store = read_accounts();
    if store.accounts.remove(email).is_none() {
        return HttpReply::error(404, &format!("no stored account for {email}"));
    }
    if let Err(e) = write_accounts(&store) {
        return HttpReply::error(500, &e);
    }
    HttpReply::ok(&serde_json::json!({ "ok": true }))
}

// ── Token refresh ────────────────────────────────────────────────────

/// If the stored token is expired but has a refresh token, attempt a
/// refresh and return updated credentials. Falls back to the original
/// on any failure.
fn maybe_refresh(mut creds: serde_json::Value) -> serde_json::Value {
    let expires_at = creds.get("expiresAt").and_then(|v| v.as_i64()).unwrap_or(0);
    if expires_at > now_ms() {
        return creds; // still valid
    }
    let refresh_token = creds.get("refreshToken").and_then(|v| v.as_str()).unwrap_or("").to_owned();
    if refresh_token.is_empty() {
        return creds; // nothing to refresh with
    }

    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": &refresh_token,
        "client_id": super::CLIENT_ID,
    });
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(super::TOKEN_URL)
        .header("Content-Type", "application/json")
        .header("User-Agent", super::TOKEN_USER_AGENT)
        .body(body.to_string())
        .timeout(std::time::Duration::from_secs(15))
        .send();

    let Ok(r) = resp else { return creds };
    if !r.status().is_success() {
        return creds;
    }
    let Ok(val) = r.json::<serde_json::Value>() else { return creds };

    let access_token = val.get("access_token").and_then(|v| v.as_str()).unwrap_or("");
    let new_refresh = val.get("refresh_token").and_then(|v| v.as_str()).unwrap_or(&refresh_token);
    let expires_in = val.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(0);
    let new_expires_at = now_ms() + expires_in * 1000;

    if !access_token.is_empty() {
        creds["accessToken"] = serde_json::Value::String(access_token.to_owned());
        creds["refreshToken"] = serde_json::Value::String(new_refresh.to_owned());
        creds["expiresAt"] = serde_json::json!(new_expires_at);
    }
    creds
}
