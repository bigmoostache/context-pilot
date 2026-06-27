//! Admin-only user-management routes (FR-04, FR-17) — list, create, force-logout
//! and delete users. Split out of [`super`] (the auth handlers/middleware) to
//! keep each file focused; all four require an authenticated `Admin` caller.

use std::sync::Mutex;

use super::{Backend, HttpReply, MIN_PASSWORD_LEN};
use crate::services::auth::types::{User, UserRole};

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
pub(crate) fn create_user(state: &Mutex<Backend>, body: &[u8], auth_user: Option<&User>) -> HttpReply {
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

/// `POST /api/auth/users/{id}/logout` — admin-only: revoke all sessions
/// for a user (force re-authentication without deleting the account).
pub(crate) fn force_logout_user(state: &Mutex<Backend>, user_id: &str, auth_user: Option<&User>) -> HttpReply {
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
    match auth.conn.execute("DELETE FROM sessions WHERE user_id = ?1", rusqlite::params![user_id]) {
        Ok(deleted) => HttpReply::ok(&serde_json::json!({
            "ok": true,
            "revoked_sessions": deleted,
        })),
        Err(_) => HttpReply::error(500, "database error"),
    }
}

/// `DELETE /api/auth/users/{id}` — admin-only: delete a user (FR-17).
/// Cascades to their sessions and ACL entries.
pub(crate) fn delete_user(state: &Mutex<Backend>, user_id: &str, auth_user: Option<&User>) -> HttpReply {
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
