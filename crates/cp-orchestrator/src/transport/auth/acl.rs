//! Per-agent ACL authorization helpers and CRUD endpoints (Phase 6).
//!
//! Split from the parent `auth` module to keep each file under 500 lines.

use std::sync::Mutex;

use super::super::Backend;
use super::super::rest::HttpReply;
use crate::services::auth::types::{User, UserRole};

// ───────────────── per-agent authorization (Phase 6) ─────────────────

/// Extract an agent id from URL segments if the route is agent-scoped.
///
/// Matches `["api", "agent", id, ..]` — every route that targets a
/// specific agent.
pub(crate) fn extract_agent_id<'a>(segments: &[&'a str]) -> Option<&'a str> {
    match segments {
        ["api", "agent", id, ..] => Some(id),
        _ => None,
    }
}

/// Check whether `user` is authorized to access `agent_id`.
///
/// System admins have implicit god-mode (FR-09). Everyone else needs an
/// explicit ACL entry (FR-10).  Returns `true` when auth is disabled
/// (backend.auth is `None`).
pub(crate) fn authorize_agent(state: &Mutex<Backend>, agent_id: &str, user: &User) -> bool {
    // System admin bypasses ACL entirely (FR-09).
    if user.role == UserRole::Admin {
        return true;
    }
    let Ok(b) = state.lock() else { return false };
    let Some(auth) = b.auth.as_ref() else { return true };
    auth.check_access(agent_id, &user.id).map(|role| role.is_some()).unwrap_or(false)
}

/// Check whether the caller can manage ACL on an agent (system admin OR
/// agent-admin on this specific agent — FR-14b/FR-14c).
fn can_manage_acl(state: &Mutex<Backend>, agent_id: &str, user: &User) -> bool {
    if user.role == UserRole::Admin {
        return true;
    }
    let Ok(b) = state.lock() else { return false };
    let Some(auth) = b.auth.as_ref() else { return false };
    auth.is_agent_admin(agent_id, &user.id).unwrap_or(false)
}

// ───────────────── ACL CRUD endpoints (Phase 6) ──────────────────────

/// `GET /api/agent/{id}/acl` — list users with access (admin or
/// agent-admin).
pub(crate) fn acl_list(state: &Mutex<Backend>, agent_id: &str, auth_user: Option<&User>) -> HttpReply {
    let Some(caller) = auth_user else {
        return HttpReply::error(501, "auth not enabled");
    };
    if !can_manage_acl(state, agent_id, caller) {
        return HttpReply::error(403, "admin or agent-admin required");
    }
    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let Some(auth) = b.auth.as_ref() else {
        return HttpReply::error(501, "auth not enabled");
    };
    match auth.list_agent_users(agent_id) {
        Ok(entries) => HttpReply::ok(&entries),
        Err(_) => HttpReply::error(500, "database error"),
    }
}

/// `POST /api/agent/{id}/acl` — grant a user access (with role).
///
/// Body: `{ "user_id": "...", "role": "agent-user" }`
pub(crate) fn acl_grant(state: &Mutex<Backend>, agent_id: &str, body: &[u8], auth_user: Option<&User>) -> HttpReply {
    let Some(caller) = auth_user else {
        return HttpReply::error(501, "auth not enabled");
    };
    if !can_manage_acl(state, agent_id, caller) {
        return HttpReply::error(403, "admin or agent-admin required");
    }

    #[derive(serde::Deserialize)]
    struct Req {
        user_id: String,
        #[serde(default = "default_agent_role")]
        role: crate::services::auth::types::AgentRole,
    }
    fn default_agent_role() -> crate::services::auth::types::AgentRole {
        crate::services::auth::types::AgentRole::AgentUser
    }

    let Ok(req) = serde_json::from_slice::<Req>(body) else {
        return HttpReply::error(400, "expected {\"user_id\":\"...\",\"role\":\"agent-user\"}");
    };

    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let Some(auth) = b.auth.as_ref() else {
        return HttpReply::error(501, "auth not enabled");
    };
    match auth.grant_access(agent_id, &req.user_id, req.role, Some(&caller.id)) {
        Ok(()) => HttpReply::ok(&serde_json::json!({ "ok": true })),
        Err(_) => HttpReply::error(500, "grant failed"),
    }
}

/// `PATCH /api/agent/{id}/acl/{userId}` — change a user's per-agent role.
///
/// Body: `{ "role": "agent-admin" }`
pub(crate) fn acl_update_role(
    state: &Mutex<Backend>,
    agent_id: &str,
    target_user_id: &str,
    body: &[u8],
    auth_user: Option<&User>,
) -> HttpReply {
    let Some(caller) = auth_user else {
        return HttpReply::error(501, "auth not enabled");
    };
    if !can_manage_acl(state, agent_id, caller) {
        return HttpReply::error(403, "admin or agent-admin required");
    }

    #[derive(serde::Deserialize)]
    struct Req {
        role: crate::services::auth::types::AgentRole,
    }
    let Ok(req) = serde_json::from_slice::<Req>(body) else {
        return HttpReply::error(400, "expected {\"role\":\"agent-admin\"}");
    };

    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let Some(auth) = b.auth.as_ref() else {
        return HttpReply::error(501, "auth not enabled");
    };
    match auth.update_agent_role(agent_id, target_user_id, req.role) {
        Ok(true) => HttpReply::ok(&serde_json::json!({ "ok": true })),
        Ok(false) => HttpReply::error(404, "access entry not found"),
        Err(_) => HttpReply::error(500, "role update failed"),
    }
}

/// `DELETE /api/agent/{id}/acl/{userId}` — revoke a user's access.
pub(crate) fn acl_revoke(
    state: &Mutex<Backend>,
    agent_id: &str,
    target_user_id: &str,
    auth_user: Option<&User>,
) -> HttpReply {
    let Some(caller) = auth_user else {
        return HttpReply::error(501, "auth not enabled");
    };
    if !can_manage_acl(state, agent_id, caller) {
        return HttpReply::error(403, "admin or agent-admin required");
    }
    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let Some(auth) = b.auth.as_ref() else {
        return HttpReply::error(501, "auth not enabled");
    };
    match auth.revoke_access(agent_id, target_user_id) {
        Ok(true) => HttpReply::ok(&serde_json::json!({ "ok": true })),
        Ok(false) => HttpReply::error(404, "access entry not found"),
        Err(_) => HttpReply::error(500, "revoke failed"),
    }
}

/// Filter a fleet of agent IDs to only those the user can access.
///
/// System admins see everything; regular users see only agents with an ACL
/// entry. When auth is disabled (`auth_user` is `None`), all agents pass.
pub(crate) fn filter_fleet(state: &Mutex<Backend>, agent_ids: &[String], auth_user: Option<&User>) -> Vec<String> {
    let Some(user) = auth_user else {
        return agent_ids.to_vec();
    };
    if user.role == UserRole::Admin {
        return agent_ids.to_vec();
    }
    let Ok(b) = state.lock() else {
        return Vec::new();
    };
    let Some(auth) = b.auth.as_ref() else {
        return agent_ids.to_vec();
    };
    let accessible = auth.list_user_agents(&user.id).unwrap_or_default();
    agent_ids.iter().filter(|id| accessible.contains(id)).cloned().collect()
}
