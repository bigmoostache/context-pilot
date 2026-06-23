//! Auth REST handlers and middleware — login, register, logout, identity,
//! user CRUD, and the centralized auth gate (Phase 5).
//!
//! All handlers return [`HttpReply`] and follow the same pattern as the
//! existing REST handlers in [`rest`](super::rest).
//!
//! The [`authenticate`] middleware runs before route dispatch for every
//! non-streaming request. When auth is disabled (`CP_AUTH_ENABLED=false`) it
//! returns `Ok(None)` immediately — zero overhead (NFR-09). When enabled, it
//! validates the Bearer token for non-public routes and returns the
//! authenticated [`User`], which handlers use for role checks.

mod acl;

pub(crate) use acl::{
    acl_grant, acl_list, acl_revoke, acl_update_role, authorize_agent,
    extract_agent_id, filter_fleet,
};

use std::sync::Mutex;

use super::rest::HttpReply;
use super::Backend;
use crate::services::auth::store::AuthStore;
use crate::services::auth::types::{User, UserRole};

/// Minimum password length (FR-21).
const MIN_PASSWORD_LEN: usize = 8;

// ─────────────────────── middleware (Phase 5) ────────────────────────

/// Authenticate the current request, returning the validated [`User`] for
/// protected routes.
///
/// Three outcomes:
/// * Auth **disabled** → `Ok(None)` (pass-through, NFR-09).
/// * Auth enabled, **public** route → `Ok(None)` (no session needed).
/// * Auth enabled, **protected** route → validates the Bearer token; returns
///   `Ok(Some(user))` on success, `Err(401)` on invalid/missing token.
///
/// Inserted in [`super::handle`] before route dispatch (NFR-16 — single
/// function, not scattered).
pub(crate) fn authenticate(
    state: &Mutex<Backend>,
    segments: &[&str],
    auth_token: Option<&str>,
) -> Result<Option<User>, HttpReply> {
    // Fast path: auth disabled — no-op pass-through (NFR-09).
    let auth_enabled = state
        .lock()
        .map(|b| b.auth.is_some())
        .unwrap_or(false);
    if !auth_enabled {
        return Ok(None);
    }

    // Public routes bypass the auth gate.
    if is_public_route(segments) {
        return Ok(None);
    }

    // Protected route — Bearer token is mandatory.
    let Some(token) = auth_token else {
        return Err(HttpReply::error(401, "missing authorization"));
    };

    let b = state
        .lock()
        .map_err(|_| HttpReply::error(500, "backend lock poisoned"))?;
    let auth = b
        .auth
        .as_ref()
        .ok_or_else(|| HttpReply::error(501, "auth not enabled"))?;

    match auth.validate_session(token) {
        Ok(Some(user)) => Ok(Some(user)),
        Ok(None) => Err(HttpReply::error(401, "invalid or expired session")),
        Err(_) => Err(HttpReply::error(500, "session validation error")),
    }
}

/// Routes that never require authentication.
fn is_public_route(segments: &[&str]) -> bool {
    matches!(
        segments,
        ["api", "health"]
            | ["api", "auth", "login"]
            | ["api", "auth", "register"]
            | ["api", "auth", "status"]
            // SSE uses ticket-based auth, not Bearer (Phase 7 enriches tickets
            // with user_id; until then the ticket mechanism is the sole gate).
            | ["api", "stream"]
    )
}

/// `GET /api/auth/status` — report whether auth is enabled and whether at
/// least one user exists (public, always accessible so the frontend can
/// decide whether to show a login vs bootstrap-register page before any
/// Bearer token is available).
pub(crate) fn auth_status(state: &Mutex<Backend>) -> HttpReply {
    let (enabled, bootstrapped) = state
        .lock()
        .map(|b| {
            let enabled = b.auth.is_some();
            let bootstrapped = b
                .auth
                .as_ref()
                .and_then(|a| a.count_users().ok())
                .map_or(false, |n| n > 0);
            (enabled, bootstrapped)
        })
        .unwrap_or((false, false));
    HttpReply::ok(&serde_json::json!({
        "enabled": enabled,
        "bootstrapped": bootstrapped,
    }))
}

// ─────────────────────── public routes ───────────────────────────────

/// `POST /api/auth/login` — authenticate with email + password, return a
/// session token and the user profile (FR-05).
///
/// Body: `{ "email": "...", "password": "..." }`
pub(crate) fn login(state: &Mutex<Backend>, body: &[u8]) -> HttpReply {
    #[derive(serde::Deserialize)]
    struct Req {
        email: String,
        password: String,
    }
    let Ok(req) = serde_json::from_slice::<Req>(body) else {
        return HttpReply::error(400, "expected {\"email\":\"...\",\"password\":\"...\"}");
    };

    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let Some(auth) = b.auth.as_ref() else {
        return HttpReply::error(501, "auth not enabled");
    };

    // Look up user by email — same error for missing user and wrong password
    // to avoid user-enumeration (NFR-03 constant-time still holds on verify).
    let user = match auth.get_user_by_email(&req.email) {
        Ok(Some(u)) => u,
        Ok(None) => return HttpReply::error(401, "invalid credentials"),
        Err(_) => return HttpReply::error(500, "database error"),
    };

    match AuthStore::verify_password(&user.password_hash, &req.password) {
        Ok(true) => {}
        Ok(false) => return HttpReply::error(401, "invalid credentials"),
        Err(_) => return HttpReply::error(500, "hash verification error"),
    }

    let ttl = b.session_ttl;
    let token = match auth.create_session(&user.id, None, ttl) {
        Ok(t) => t,
        Err(_) => return HttpReply::error(500, "session creation failed"),
    };

    HttpReply::ok(&serde_json::json!({
        "token": token,
        "user": user,
    }))
}

/// `POST /api/auth/register` — bootstrap-only (zero users → admin) or
/// admin-creates-user (FR-03, FR-04).
///
/// Body: `{ "email": "...", "name": "...", "password": "..." }`
///
/// This route is "semi-public": the auth middleware lets it through without
/// a token so the first-user bootstrap works (FR-03). When users already
/// exist, the handler requires an admin session (checked via `auth_user`).
pub(crate) fn register(
    state: &Mutex<Backend>,
    body: &[u8],
    auth_user: Option<&User>,
) -> HttpReply {
    #[derive(serde::Deserialize)]
    struct Req {
        email: String,
        name: String,
        password: String,
    }
    let Ok(req) = serde_json::from_slice::<Req>(body) else {
        return HttpReply::error(400, "expected {\"email\":\"...\",\"name\":\"...\",\"password\":\"...\"}");
    };

    if req.password.len() < MIN_PASSWORD_LEN {
        return HttpReply::error(400, "password must be at least 8 characters");
    }

    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let Some(auth) = b.auth.as_ref() else {
        return HttpReply::error(501, "auth not enabled");
    };

    let user_count = match auth.count_users() {
        Ok(n) => n,
        Err(_) => return HttpReply::error(500, "database error"),
    };

    // Bootstrap: first user becomes admin (FR-03).
    // Subsequent registrations require a valid admin session.
    let role = if user_count == 0 {
        UserRole::Admin
    } else {
        match auth_user {
            Some(u) if u.role == UserRole::Admin => UserRole::User,
            Some(_) => return HttpReply::error(403, "admin access required"),
            None => return HttpReply::error(401, "admin authorization required"),
        }
    };

    let user = match auth.create_user(&req.email, &req.name, &req.password, role) {
        Ok(u) => u,
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("UNIQUE") {
                return HttpReply::error(409, "email already registered");
            }
            return HttpReply::error(500, "user creation failed");
        }
    };

    HttpReply::ok(&serde_json::json!({ "user": user }))
}

// ─────────────────── protected routes ────────────────────────────────

/// `POST /api/auth/logout` — destroy the current session (FR-06).
///
/// The middleware already validated the session; we just need the raw token
/// to revoke it.
pub(crate) fn logout(state: &Mutex<Backend>, auth_token: Option<&str>) -> HttpReply {
    let Some(token) = auth_token else {
        return HttpReply::error(401, "missing authorization");
    };
    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let Some(auth) = b.auth.as_ref() else {
        return HttpReply::error(501, "auth not enabled");
    };
    match auth.revoke_session(token) {
        Ok(true) => HttpReply::ok(&serde_json::json!({ "ok": true })),
        Ok(false) => HttpReply::error(401, "invalid or expired session"),
        Err(_) => HttpReply::error(500, "database error"),
    }
}

/// `GET /api/auth/me` — current user profile (FR-07).
///
/// The middleware guarantees `auth_user` is `Some` when auth is enabled.
pub(crate) fn me(auth_user: Option<&User>) -> HttpReply {
    match auth_user {
        Some(user) => HttpReply::ok(user),
        None => HttpReply::error(501, "auth not enabled"),
    }
}

/// `GET /api/auth/users` — admin-only: list all users (FR-04).
pub(crate) fn list_users(state: &Mutex<Backend>, auth_user: Option<&User>) -> HttpReply {
    let Some(caller) = auth_user else {
        return HttpReply::error(501, "auth not enabled");
    };
    if caller.role != UserRole::Admin {
        return HttpReply::error(403, "admin access required");
    }
    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let Some(auth) = b.auth.as_ref() else {
        return HttpReply::error(501, "auth not enabled");
    };
    match auth.list_users() {
        Ok(users) => HttpReply::ok(&users),
        Err(_) => HttpReply::error(500, "database error"),
    }
}

/// `POST /api/auth/users` — admin-only: create a new user (FR-04).
///
/// Body: `{ "email": "...", "name": "...", "password": "...", "role": "user" }`
pub(crate) fn create_user(
    state: &Mutex<Backend>,
    body: &[u8],
    auth_user: Option<&User>,
) -> HttpReply {
    let Some(caller) = auth_user else {
        return HttpReply::error(501, "auth not enabled");
    };
    if caller.role != UserRole::Admin {
        return HttpReply::error(403, "admin access required");
    }

    #[derive(serde::Deserialize)]
    struct Req {
        email: String,
        name: String,
        password: String,
        #[serde(default = "default_user_role")]
        role: UserRole,
    }
    fn default_user_role() -> UserRole {
        UserRole::User
    }

    let Ok(req) = serde_json::from_slice::<Req>(body) else {
        return HttpReply::error(400, "expected {\"email\":\"...\",\"name\":\"...\",\"password\":\"...\"}");
    };
    if req.password.len() < MIN_PASSWORD_LEN {
        return HttpReply::error(400, "password must be at least 8 characters");
    }

    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let Some(auth) = b.auth.as_ref() else {
        return HttpReply::error(501, "auth not enabled");
    };
    match auth.create_user(&req.email, &req.name, &req.password, req.role) {
        Ok(user) => HttpReply::ok(&serde_json::json!({ "user": user })),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("UNIQUE") {
                return HttpReply::error(409, "email already registered");
            }
            HttpReply::error(500, "user creation failed")
        }
    }
}

/// `DELETE /api/auth/users/{id}` — admin-only: delete a user (FR-17).
/// Cascades to their sessions and ACL entries.
pub(crate) fn delete_user(
    state: &Mutex<Backend>,
    user_id: &str,
    auth_user: Option<&User>,
) -> HttpReply {
    let Some(caller) = auth_user else {
        return HttpReply::error(501, "auth not enabled");
    };
    if caller.role != UserRole::Admin {
        return HttpReply::error(403, "admin access required");
    }
    // Prevent admin from deleting themselves.
    if caller.id == user_id {
        return HttpReply::error(400, "cannot delete yourself");
    }

    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let Some(auth) = b.auth.as_ref() else {
        return HttpReply::error(501, "auth not enabled");
    };
    match auth.delete_user(user_id) {
        Ok(true) => HttpReply::ok(&serde_json::json!({ "ok": true })),
        Ok(false) => HttpReply::error(404, "user not found"),
        Err(_) => HttpReply::error(500, "database error"),
    }
}
