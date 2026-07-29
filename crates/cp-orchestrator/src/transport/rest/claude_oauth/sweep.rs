//! Background OAuth refresh sweep — keeps EVERY Claude account (the active slot
//! plus every stored account) within a fresh token, without any UI being open.
//!
//! A single standalone thread wakes every [`SWEEP_INTERVAL`], and for each
//! account whose token is within [`accounts::REFRESH_THRESHOLD_MS`] (1 hour) of
//! expiry, exchanges its refresh token for a new one. It needs nothing from
//! [`Backend`](crate::transport::Backend) — all OAuth state lives on disk / in
//! the Keychain — so it takes no locks and is spawned directly (see
//! [`crate::runtime`]).
//!
//! This supersedes the frontend's popover-scoped auto-refresh: the server keeps
//! tokens alive continuously, so no user action (or open tab) is required.

use std::thread;
use std::time::Duration;

use super::accounts::{self, REFRESH_THRESHOLD_MS};

/// How often the sweep runs. A token entering its final hour is therefore
/// renewed within at most one interval — well before it can expire.
const SWEEP_INTERVAL: Duration = Duration::from_secs(600);

/// Settle delay before the first sweep — lets the transport bind and the boot
/// sequence land so a refresh never races startup.
const BOOT_DELAY: Duration = Duration::from_secs(10);

/// Spawn the OAuth refresh sweeper. One thread for the process lifetime.
pub(crate) fn spawn() -> thread::JoinHandle<()> {
    thread::spawn(|| {
        thread::sleep(BOOT_DELAY);
        loop {
            sweep_once();
            thread::sleep(SWEEP_INTERVAL);
        }
    })
}

/// One pass: refresh the active-slot token if stale, then every stored account.
fn sweep_once() {
    refresh_active_if_stale();
    accounts::refresh_stored_accounts(REFRESH_THRESHOLD_MS);
}

/// Refresh the ACTIVE (Keychain / `~/.claude/.credentials.json`) token when it
/// is within an hour of expiry, writing the renewed blob back. Best-effort — a
/// failed refresh leaves the current credentials in place (the access token may
/// still have runway; the switch/login flow re-authenticates once it expires).
fn refresh_active_if_stale() {
    let Some(creds) = super::read_credentials_json() else { return };
    if !accounts::is_stale(&creds, REFRESH_THRESHOLD_MS) {
        return;
    }
    let refresh_token = creds.get("refreshToken").and_then(|v| v.as_str()).unwrap_or("").to_owned();
    if let Some(fresh) = accounts::try_refresh(&creds, &refresh_token) {
        let wrapped = serde_json::json!({ "claudeAiOauth": fresh });
        let _w = super::store_credentials(&wrapped);
    }
}
