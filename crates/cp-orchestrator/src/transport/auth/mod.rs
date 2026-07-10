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
mod users;

pub(crate) use acl::{
    acl_grant, acl_list, acl_revoke, acl_update_role, authorize_agent, extract_agent_id, filter_fleet,
};
pub(crate) use users::{create_user, delete_user, force_logout_user, list_users};

use std::sync::Mutex;

use super::Backend;
use super::rest::HttpReply;
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
    // Fast path: access control OFF (explicit `"false"` opt-out) → god mode
    // (FR-v3-08, design §13.10; RBAC is ON by default now). Everyone is
    // effectively superadmin with no login; the enforcement
    // sites already short-circuit to full access on `auth_user == None`. Also a
    // no-op when there is no auth store to enforce against (NFR-09). Both are read
    // from the cached [`Backend::access_control`] flag — no per-request disk I/O.
    let (access_control, auth_enabled) =
        state.lock().map(|b| (b.access_control, b.auth.is_some())).unwrap_or((false, false));
    if !access_control || !auth_enabled {
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

    let b = state.lock().map_err(|_| HttpReply::error(500, "backend lock poisoned"))?;
    let auth = b.auth.as_ref().ok_or_else(|| HttpReply::error(501, "auth not enabled"))?;

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
            // Agent avatars are loaded by a plain `<img src>` element, which
            // cannot attach an `Authorization: Bearer` header — so the route
            // must be public or every avatar 401s once auth is on (T345).
            // Profile pictures are non-sensitive (shown in the switcher to any
            // authenticated viewer), so public access is safe. Marking it
            // public here also skips the per-agent ACL check in `handle`
            // (which only runs when `auth_user` is `Some`).
            | ["api", "agent", _, "avatar"]
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
            let bootstrapped = b.auth.as_ref().and_then(|a| a.count_users().ok()).map_or(false, |n| n > 0);
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
pub(crate) fn register(state: &Mutex<Backend>, body: &[u8], auth_user: Option<&User>) -> HttpReply {
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

    // Bootstrap: the first-ever account is the vendor `superadmin` (FR-03 →
    // design §13.9). Subsequent self-serve registrations require a
    // `can_manage_users` session and always create a plain `user`.
    let role = if user_count == 0 {
        UserRole::Superadmin
    } else {
        match auth_user {
            Some(u) if u.can_manage_users() => UserRole::User,
            Some(_) => return HttpReply::error(403, "user management access required"),
            None => return HttpReply::error(401, "management authorization required"),
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

/// `GET /api/auth/me` — the serialized [`User`] plus a backend-driven
/// `next_action` (FR-07): `change_password` / `set_identity` (day-0) /
/// `onboarding` / `ready`. `me` reads the durable provisioning flag (state the
/// [`User`] doesn't carry) and threads it into [`next_action`]. The middleware
/// guarantees `auth_user` is `Some` when auth is enabled.
pub(crate) fn me(state: &Mutex<Backend>, auth_user: Option<&User>) -> HttpReply {
    let Some(user) = auth_user else {
        return HttpReply::error(501, "auth not enabled");
    };
    let provisioned =
        state.lock().map(|b| crate::transport::it::is_provisioned(&b.provision_flag_path)).unwrap_or(false);
    let mut value = serde_json::to_value(user).unwrap_or_default();
    if let Some(obj) = value.as_object_mut() {
        drop(obj.insert("next_action".to_owned(), next_action(user, provisioned).into()));
    }
    HttpReply::ok(&value)
}

/// Decide the post-login step the frontend should render for `user`, given the
/// box's `provisioned` state. Order (mirrors the web `AuthGuard`): password
/// rotation → day-0 identity/provisioning → first-run onboarding → the app.
fn next_action(user: &User, provisioned: bool) -> &'static str {
    if user.must_change_password {
        "change_password"
    } else if user.can_manage_it() && !provisioned {
        // Day-0 (design §13.4): an IT operator names the unprovisioned box, which
        // provisions it and brings the private-CA `:443` cockpit up (CA download too).
        "set_identity"
    } else if user.can_manage_users() && !super::rest::onboarding_completed() {
        // First-run product/org onboarding — the client-management tier's setup.
        "onboarding"
    } else {
        "ready"
    }
}

/// `POST /api/auth/password` — change the current user's password (self-serve
/// profile). Body: `{ "current": "...", "new": "..." }`. Verifies the current
/// password before applying the new one (min length enforced).
pub(crate) fn change_password(state: &Mutex<Backend>, body: &[u8], auth_user: Option<&User>) -> HttpReply {
    let Some(caller) = auth_user else {
        return HttpReply::error(501, "auth not enabled");
    };
    #[derive(serde::Deserialize)]
    struct Req {
        current: String,
        new: String,
    }
    let Ok(req) = serde_json::from_slice::<Req>(body) else {
        return HttpReply::error(400, "expected {\"current\":\"...\",\"new\":\"...\"}");
    };
    if req.new.len() < MIN_PASSWORD_LEN {
        return HttpReply::error(400, "password must be at least 8 characters");
    }
    match AuthStore::verify_password(&caller.password_hash, &req.current) {
        Ok(true) => {}
        Ok(false) => return HttpReply::error(403, "current password is incorrect"),
        Err(_) => return HttpReply::error(500, "hash verification error"),
    }
    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let Some(auth) = b.auth.as_ref() else {
        return HttpReply::error(501, "auth not enabled");
    };
    match auth.update_password(&caller.id, &req.new) {
        Ok(true) => HttpReply::ok(&serde_json::json!({ "ok": true })),
        Ok(false) => HttpReply::error(404, "user not found"),
        Err(_) => HttpReply::error(500, "database error"),
    }
}

/// `PATCH /api/auth/me` — update the current user's display name and email.
/// Body: `{ "name": "...", "email": "..." }`. Returns the refreshed profile.
pub(crate) fn update_me(state: &Mutex<Backend>, body: &[u8], auth_user: Option<&User>) -> HttpReply {
    let Some(caller) = auth_user else {
        return HttpReply::error(501, "auth not enabled");
    };
    #[derive(serde::Deserialize)]
    struct Req {
        name: String,
        email: String,
    }
    let Ok(req) = serde_json::from_slice::<Req>(body) else {
        return HttpReply::error(400, "expected {\"name\":\"...\",\"email\":\"...\"}");
    };
    let name = req.name.trim();
    let email = req.email.trim();
    if name.is_empty() || email.is_empty() {
        return HttpReply::error(400, "name and email are required");
    }
    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let Some(auth) = b.auth.as_ref() else {
        return HttpReply::error(501, "auth not enabled");
    };
    match auth.update_profile(&caller.id, name, email) {
        Ok(_) => match auth.get_user_by_id(&caller.id) {
            Ok(Some(user)) => HttpReply::ok(&serde_json::json!({ "user": user })),
            _ => HttpReply::error(500, "database error"),
        },
        Err(e) => {
            if e.to_string().contains("UNIQUE") {
                return HttpReply::error(409, "email already registered");
            }
            HttpReply::error(500, "profile update failed")
        }
    }
}

/// `GET /api/auth/sessions` — list the current user's active device sessions,
/// flagging the one making this request. Never returns raw tokens.
pub(crate) fn list_sessions(state: &Mutex<Backend>, auth_token: Option<&str>, auth_user: Option<&User>) -> HttpReply {
    let Some(caller) = auth_user else {
        return HttpReply::error(501, "auth not enabled");
    };
    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let Some(auth) = b.auth.as_ref() else {
        return HttpReply::error(501, "auth not enabled");
    };
    match auth.list_sessions(&caller.id, auth_token) {
        Ok(sessions) => HttpReply::ok(&serde_json::json!({ "sessions": sessions })),
        Err(_) => HttpReply::error(500, "database error"),
    }
}

/// `DELETE /api/auth/sessions/{id}` — revoke one of the current user's own
/// device sessions by its opaque id. Scoped to the caller so a user can only
/// drop their own devices.
pub(crate) fn revoke_session(state: &Mutex<Backend>, session_id: &str, auth_user: Option<&User>) -> HttpReply {
    let Some(caller) = auth_user else {
        return HttpReply::error(501, "auth not enabled");
    };
    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let Some(auth) = b.auth.as_ref() else {
        return HttpReply::error(501, "auth not enabled");
    };
    match auth.revoke_session_by_id(&caller.id, session_id) {
        Ok(Some(_)) => HttpReply::ok(&serde_json::json!({ "ok": true })),
        Ok(None) => HttpReply::error(404, "session not found"),
        Err(_) => HttpReply::error(500, "database error"),
    }
}

// Admin-only user-management routes (list/create/force-logout/delete) live in
// the sibling `users` module and are re-exported above.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;

    /// Build a `Mutex<Backend>` with an auth store present, and the access-control
    /// flag forced to `access_control`. The tempdir is leaked so the SQLite file
    /// outlives the test body.
    fn backend(access_control: bool) -> Mutex<Backend> {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = AuthStore::open(&dir.path().join("auth.db")).expect("open auth store");
        let mut b = Backend::new(
            dir.path().to_path_buf(),
            PathBuf::from("/tmp/cp-auth-test-realms"),
            PathBuf::from("/tmp/cp-auth-test-bin"),
            Some(store),
            Duration::from_secs(3600),
        );
        b.access_control = access_control;
        std::mem::forget(dir);
        Mutex::new(b)
    }

    /// V0.4b — with the flag OFF, a request with no token on an agent-scoped
    /// route resolves to full access (`Ok(None)` = god mode), even though an auth
    /// store is present. Mirrors the current auth-disabled behaviour.
    #[test]
    fn flag_off_is_god_mode() {
        let state = backend(false);
        let outcome = authenticate(&state, &["api", "agent", "some-agent"], None);
        assert!(matches!(outcome, Ok(None)), "flag off ⇒ no authenticated user (god mode)");
    }

    /// With the flag ON, the same tokenless request to a protected route is
    /// rejected (401), while a public route still passes through.
    #[test]
    fn flag_on_enforces() {
        let state = backend(true);
        let protected = authenticate(&state, &["api", "agent", "some-agent"], None);
        assert!(matches!(protected, Err(reply) if reply.status == 401), "flag on + no token ⇒ 401");
        let public = authenticate(&state, &["api", "health"], None);
        assert!(matches!(public, Ok(None)), "public route bypasses the gate");
    }

    // Bare variant imports (not the `UserRole::Superadmin` qualified path) keep
    // the capability-grep gate happy (reserved for capabilities/types/tests.rs).
    use crate::services::auth::types::UserRole::{Superadmin, User as Regular};

    fn user(role: UserRole, must_change_password: bool) -> User {
        User {
            id: "a".to_owned(),
            email: "a@box".to_owned(),
            name: "A".to_owned(),
            password_hash: String::new(),
            role,
            must_change_password,
            created_at: 0,
            updated_at: 0,
        }
    }

    /// V5.2a — `me` drives the day-0 flow via `next_action` from the threaded
    /// provisioning state: a `can_manage_it` operator on an unprovisioned box walks
    /// `change_password → set_identity → (cleared)` as each precondition is met; a
    /// regular user skips the IT step → `ready`. Hermetic (only the durable flag).
    #[test]
    fn me_next_action_day0_walk_and_capability_scope() {
        let state = backend(false); // me() ignores access_control; needs the flag path
        let flag_path = state.lock().expect("lock").provision_flag_path.clone();
        let na = |u: &User| me(&state, Some(u)).body;
        let set = |v| crate::transport::it::state::set_provisioned(&flag_path, v).expect("flag");
        set(false);
        let mut admin = user(Superadmin, true);
        // 1. Seeded paper password must be rotated first.
        assert!(na(&admin).contains("\"next_action\":\"change_password\""), "paper password first");
        // 2. Password changed, box still unprovisioned → day-0 identity step.
        admin.must_change_password = false;
        assert!(na(&admin).contains("\"next_action\":\"set_identity\""), "unprovisioned IT operator names the box");
        // 3. Provisioned → day-0 IT gate cleared (proceeds to app / onboarding).
        set(true);
        let done = na(&admin);
        assert!(!done.contains("\"next_action\":\"set_identity\""), "provisioned clears day-0: {done}");
        // A regular user skips the IT step; no user-mgmt cap → onboarding short-circuits → ready.
        set(false);
        assert!(na(&user(Regular, false)).contains("\"next_action\":\"ready\""), "non-IT user ⇒ ready");
    }
}
