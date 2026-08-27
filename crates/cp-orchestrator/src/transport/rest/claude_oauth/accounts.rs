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

/// Refresh a token once it drops within this window of expiry (1 hour). Shared
/// by the account-switch path and the background sweep so both use one policy.
pub(super) const REFRESH_THRESHOLD_MS: i64 = 3_600_000;

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
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
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
            let expires_at = creds.get("expiresAt").and_then(serde_json::Value::as_i64);
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

    // If the access token is expired or within an hour of expiry, refresh it
    // before activating so the switched-to account is comfortably usable.
    let target_creds = maybe_refresh(target_creds, REFRESH_THRESHOLD_MS);

    // Save current active into the store (best-effort: if no active token
    // exists we still proceed with the switch).
    if let Some(current) = super::read_credentials_json() {
        let current_token = current.get("accessToken").and_then(|v| v.as_str()).unwrap_or("");
        if !current_token.is_empty()
            && let Some(current_email) = super::fetch_account_email(current_token) {
                let _prev = store.accounts.insert(current_email, current);
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

/// True when `creds` will expire within `threshold_ms` (or already has) AND
/// carries a non-empty refresh token to renew with. `threshold_ms == 0` is the
/// classic "expired only" test; a positive value (e.g. 1h) refreshes early.
pub(super) fn is_stale(creds: &serde_json::Value, threshold_ms: i64) -> bool {
    let expires_at = creds.get("expiresAt").and_then(serde_json::Value::as_i64).unwrap_or(0);
    let has_refresh = creds.get("refreshToken").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty());
    has_refresh && expires_at.saturating_sub(now_ms()) < threshold_ms
}

/// Exchange a refresh token for a fresh `claudeAiOauth` credential blob.
///
/// The single shared refresh-grant POST (previously copy-pasted across the
/// switch and login paths). Returns the new `{accessToken, refreshToken,
/// expiresAt}` fields folded onto `base`, or `None` on any failure (network,
/// non-2xx, empty access token) so callers keep the old credentials intact.
pub(super) fn try_refresh(base: &serde_json::Value, refresh_token: &str) -> Option<serde_json::Value> {
    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": super::CLIENT_ID,
    });
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(super::TOKEN_URL)
        .header("Content-Type", "application/json")
        .header("User-Agent", super::TOKEN_USER_AGENT)
        .body(body.to_string())
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let val = resp.json::<serde_json::Value>().ok()?;

    let access_token = val.get("access_token").and_then(|v| v.as_str()).unwrap_or("");
    if access_token.is_empty() {
        return None;
    }
    let new_refresh = val.get("refresh_token").and_then(|v| v.as_str()).unwrap_or(refresh_token);
    let expires_in = val.get("expires_in").and_then(serde_json::Value::as_i64).unwrap_or(0);

    let mut creds = base.clone();
    creds["accessToken"] = serde_json::Value::String(access_token.to_owned());
    creds["refreshToken"] = serde_json::Value::String(new_refresh.to_owned());
    creds["expiresAt"] = serde_json::json!(now_ms() + expires_in * 1000);
    Some(creds)
}

/// If `creds` is stale (within `threshold_ms` of expiry), attempt a refresh and
/// return the updated blob. Falls back to the original on any failure.
fn maybe_refresh(creds: serde_json::Value, threshold_ms: i64) -> serde_json::Value {
    if !is_stale(&creds, threshold_ms) {
        return creds;
    }
    let refresh_token = creds.get("refreshToken").and_then(|v| v.as_str()).unwrap_or("").to_owned();
    try_refresh(&creds, &refresh_token).unwrap_or(creds)
}

/// Refresh every STORED account whose token is within `threshold_ms` of expiry,
/// rewriting `claude-accounts.json` only when at least one changed. Best-effort:
/// an individual refresh failure leaves that account's credentials untouched.
/// The active-slot token is refreshed separately (see `sweep`).
pub(super) fn refresh_stored_accounts(threshold_ms: i64) {
    let mut store = read_accounts();
    let mut dirty = false;
    for creds in store.accounts.values_mut() {
        if !is_stale(creds, threshold_ms) {
            continue;
        }
        let refresh_token = creds.get("refreshToken").and_then(|v| v.as_str()).unwrap_or("").to_owned();
        if let Some(fresh) = try_refresh(creds, &refresh_token) {
            *creds = fresh;
            dirty = true;
        }
    }
    if dirty {
        let _w = write_accounts(&store);
    }
}
