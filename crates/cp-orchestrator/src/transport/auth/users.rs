//! User-management routes (FR-04, FR-17, design §13.3/§13.7) — list, create,
//! force-logout and delete users. Split out of [`super`] (the auth
//! handlers/middleware) to keep each file focused.
//!
//! All four require `can_manage_users` (manager+). On top of that gate two
//! cross-cutting rules apply (design §13.3):
//!
//! * **Vendor invisibility** (FR-v3-05, [`User::can_see`]) — a `superadmin`
//!   account is hidden from, and non-manageable by, any non-superadmin caller.
//!   It is filtered from list responses and answered with `404` (not `403`) on
//!   direct address so its very existence is not disclosed.
//! * **Anti-escalation** (FR-v3-03, [`User::can_assign_role`]) — a caller may
//!   only create/target an account of **strictly lower** rank than their own.

use std::sync::Mutex;

use super::{Backend, HttpReply, MIN_PASSWORD_LEN};
use crate::services::auth::types::{User, UserRole};

/// `GET /api/auth/users` — list all users the caller may see (FR-04). Superadmin
/// rows are filtered out for non-superadmin callers (FR-v3-05).
pub(crate) fn list_users(state: &Mutex<Backend>, auth_user: Option<&User>) -> HttpReply {
    let Some(caller) = auth_user else {
        return HttpReply::error(501, "auth not enabled");
    };
    if !caller.can_manage_users() {
        return HttpReply::error(403, "user management access required");
    }
    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let Some(auth) = b.auth.as_ref() else {
        return HttpReply::error(501, "auth not enabled");
    };
    match auth.list_users() {
        Ok(users) => {
            let visible: Vec<User> = users.into_iter().filter(|u| caller.can_see(u.role)).collect();
            HttpReply::ok(&visible)
        }
        Err(_) => HttpReply::error(500, "database error"),
    }
}

/// `POST /api/auth/users` — create a new user (FR-04) of a role strictly below
/// the caller's own (FR-v3-03).
///
/// Body: `{ "email": "...", "name": "...", "password": "...", "role": "user" }`
pub(crate) fn create_user(state: &Mutex<Backend>, body: &[u8], auth_user: Option<&User>) -> HttpReply {
    let Some(caller) = auth_user else {
        return HttpReply::error(501, "auth not enabled");
    };
    if !caller.can_manage_users() {
        return HttpReply::error(403, "user management access required");
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
    // Anti-escalation: nobody creates a peer or a superior (FR-v3-03).
    if !caller.can_assign_role(req.role) {
        return HttpReply::error(403, "cannot assign a role at or above your own");
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

/// `POST /api/auth/users/{id}/logout` — revoke all sessions for a user (force
/// re-authentication without deleting the account). Requires `can_manage_users`
/// and that the target is visible + of strictly lower rank.
pub(crate) fn force_logout_user(state: &Mutex<Backend>, user_id: &str, auth_user: Option<&User>) -> HttpReply {
    let Some(caller) = auth_user else {
        return HttpReply::error(501, "auth not enabled");
    };
    if !caller.can_manage_users() {
        return HttpReply::error(403, "user management access required");
    }
    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let Some(auth) = b.auth.as_ref() else {
        return HttpReply::error(501, "auth not enabled");
    };
    match target_manageable_by(auth, caller, user_id) {
        TargetCheck::Ok => {}
        TargetCheck::NotFound => return HttpReply::error(404, "user not found"),
        TargetCheck::Forbidden => return HttpReply::error(403, "cannot manage a user at or above your own rank"),
        TargetCheck::DbError => return HttpReply::error(500, "database error"),
    }
    match auth.conn.execute("DELETE FROM sessions WHERE user_id = ?1", rusqlite::params![user_id]) {
        Ok(deleted) => HttpReply::ok(&serde_json::json!({
            "ok": true,
            "revoked_sessions": deleted,
        })),
        Err(_) => HttpReply::error(500, "database error"),
    }
}

/// `DELETE /api/auth/users/{id}` — delete a user (FR-17). Cascades to their
/// sessions and ACL entries. Requires `can_manage_users` and that the target is
/// visible + of strictly lower rank (FR-v3-03/05).
pub(crate) fn delete_user(state: &Mutex<Backend>, user_id: &str, auth_user: Option<&User>) -> HttpReply {
    let Some(caller) = auth_user else {
        return HttpReply::error(501, "auth not enabled");
    };
    if !caller.can_manage_users() {
        return HttpReply::error(403, "user management access required");
    }
    // Prevent deleting yourself.
    if caller.id == user_id {
        return HttpReply::error(400, "cannot delete yourself");
    }

    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let Some(auth) = b.auth.as_ref() else {
        return HttpReply::error(501, "auth not enabled");
    };
    match target_manageable_by(auth, caller, user_id) {
        TargetCheck::Ok => {}
        TargetCheck::NotFound => return HttpReply::error(404, "user not found"),
        TargetCheck::Forbidden => return HttpReply::error(403, "cannot delete a user at or above your own rank"),
        TargetCheck::DbError => return HttpReply::error(500, "database error"),
    }
    match auth.delete_user(user_id) {
        Ok(true) => HttpReply::ok(&serde_json::json!({ "ok": true })),
        Ok(false) => HttpReply::error(404, "user not found"),
        Err(_) => HttpReply::error(500, "database error"),
    }
}

/// Outcome of the visibility + anti-escalation check on a management target.
enum TargetCheck {
    /// Caller may manage the target.
    Ok,
    /// Target does not exist, or is invisible to the caller (FR-v3-05) — both
    /// answered `404` so existence is never disclosed.
    NotFound,
    /// Target is visible but at or above the caller's rank (FR-v3-03).
    Forbidden,
    /// Database failure while resolving the target.
    DbError,
}

/// May `caller` manage the account `target_id`? Enforces vendor invisibility
/// then anti-escalation. A caller can only act on a **visible** account of
/// **strictly lower** rank; an invisible one is reported as [`NotFound`] so a
/// non-superadmin cannot probe for superadmin accounts.
///
/// [`NotFound`]: TargetCheck::NotFound
fn target_manageable_by(auth: &crate::services::auth::store::AuthStore, caller: &User, target_id: &str) -> TargetCheck {
    let target = match auth.get_user_by_id(target_id) {
        Ok(Some(u)) => u,
        Ok(None) => return TargetCheck::NotFound,
        Err(_) => return TargetCheck::DbError,
    };
    if !caller.can_see(target.role) {
        return TargetCheck::NotFound;
    }
    if !caller.can_assign_role(target.role) {
        return TargetCheck::Forbidden;
    }
    TargetCheck::Ok
}
