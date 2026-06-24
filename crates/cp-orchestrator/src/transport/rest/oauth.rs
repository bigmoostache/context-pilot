//! Claude Code OAuth manual-login REST handlers (P-oauth).
//!
//! Two gated routes drive the paste flow: [`start`] hands the browser an
//! authorize URL and remembers the PKCE verifier; [`finish`] takes the pasted
//! `code#state`, exchanges it, and writes `~/.claude/.credentials.json`. The
//! token refresh that keeps it alive on a headless host runs separately on the
//! runtime's background refresher.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::HttpReply;
use crate::services::auth::types::{User, UserRole};
use crate::services::claude_oauth;
use crate::transport::Backend;

/// The Claude Code OAuth credentials are central (shared by all agents), so
/// only an admin may (re)connect them. Open on a single-user appliance (auth
/// disabled).
fn require_admin(state: &Mutex<Backend>, auth_user: Option<&User>) -> bool {
    let auth_enabled = state.lock().map(|b| b.auth.is_some()).unwrap_or(false);
    if !auth_enabled {
        return true;
    }
    matches!(auth_user, Some(u) if u.role == UserRole::Admin)
}

/// `POST /api/auth/oauth/start` — begin a manual login; returns the authorize
/// URL for the browser. Stashes the PKCE pending state server-side.
pub fn start(state: &Mutex<Backend>, auth_user: Option<&User>) -> HttpReply {
    if !require_admin(state, auth_user) {
        return HttpReply::error(403, "admin access required");
    }
    let (url, pending) = match claude_oauth::start() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("oauth/start: build authorize url failed: {e}");
            return HttpReply::error(500, &format!("oauth start failed: {e}"));
        }
    };
    let Ok(mut backend) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let replaced = backend.pending_oauth.is_some();
    backend.pending_oauth = Some(pending);
    eprintln!("oauth/start: pending stored (replaced existing: {replaced})");
    HttpReply::ok(&StartOut { authorize_url: url })
}

/// `POST /api/auth/oauth/finish` — complete the login with the pasted
/// `code#state`. Exchanges for tokens and writes the credentials file.
pub fn finish(state: &Mutex<Backend>, body_bytes: &[u8], auth_user: Option<&User>) -> HttpReply {
    if !require_admin(state, auth_user) {
        return HttpReply::error(403, "admin access required");
    }
    let Ok(req) = serde_json::from_slice::<FinishReq>(body_bytes) else {
        eprintln!("oauth/finish: malformed request body");
        return HttpReply::error(400, "malformed oauth request");
    };
    if req.code.trim().is_empty() {
        eprintln!("oauth/finish: empty code");
        return HttpReply::error(400, "code is required");
    }
    let pasted = req.code.trim();
    eprintln!(
        "oauth/finish: received code (len={}, has_hash={})",
        pasted.len(),
        pasted.contains('#')
    );

    // Take the pending state out under the lock; the (slow) network exchange
    // then runs lock-free.
    let pending = {
        let Ok(mut backend) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        backend.pending_oauth.take()
    };
    let Some(pending) = pending else {
        eprintln!("oauth/finish: no pending login (start was never called or already consumed)");
        return HttpReply::error(409, "no oauth login in progress");
    };

    let creds = match claude_oauth::exchange(&req.code, &pending) {
        Ok(c) => c,
        Err(e) if e == "state mismatch" => {
            eprintln!("oauth/finish: state mismatch (pasted state != pending state)");
            return HttpReply::error(400, "state mismatch");
        }
        Err(e) => {
            eprintln!("oauth/finish: exchange failed: {e}");
            return HttpReply::error(502, &format!("oauth exchange failed: {e}"));
        }
    };
    if let Err(e) = claude_oauth::write_credentials(&creds) {
        eprintln!("oauth/finish: write_credentials failed: {e}");
        return HttpReply::error(500, &format!("could not write credentials: {e}"));
    }
    eprintln!("oauth/finish: success — credentials written");
    HttpReply::ok(&AckOut { ok: true })
}

/// `POST /api/auth/oauth/start` response.
#[derive(Serialize)]
struct StartOut {
    /// The Anthropic authorize URL to open in the browser.
    authorize_url: String,
}

/// `POST /api/auth/oauth/finish` request.
#[derive(Deserialize)]
struct FinishReq {
    /// The pasted `code#state` (or bare `code`).
    code: String,
}

/// Generic boolean acknowledgement.
#[derive(Serialize)]
struct AckOut {
    /// Whether the operation took effect.
    ok: bool,
}
