//! Central cockpit settings — server-side new-agent defaults, the onboarding
//! flag, and provider API-key management.
//!
//! Defaults and the onboarding flag live in the shared global config
//! (`~/.config/context-pilot/config.json` via [`cp_base::config::global`]),
//! the same file agents read their provider keys from — so the admin sets a
//! default provider/model + keys once at onboarding and every agent picks them
//! up. Reads are available to any authenticated user (regular users render the
//! defaults read-only); writes require an admin when auth is enabled, and are
//! open on a single-user appliance (auth disabled).

use std::sync::Mutex;

use cp_base::config::global;

use super::super::{Backend, HttpReply};
use crate::services::auth::types::{User, UserRole};

/// Canonical LLM provider key names surfaced in the cockpit onboarding/profile.
const LLM_PROVIDERS: &[&str] = &["anthropic", "deepseek", "xai", "groq"];

/// Setting keys stored in the central config.
const DEFAULT_PROVIDER: &str = "default_provider";
const DEFAULT_MODEL: &str = "default_model";
const ONBOARDING_DONE: &str = "onboarding_completed";
/// JSON array of `"<providerId>:<modelId>"` the admin permits org-wide. An
/// **empty** list means *all models allowed* (non-blocking default at delivery).
const ALLOWED_MODELS: &str = "allowed_models";

/// Read the org-wide allowed-model allowlist (empty when unset / unparsable).
fn allowed_models() -> Vec<String> {
    global::get_setting(ALLOWED_MODELS).and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok()).unwrap_or_default()
}

/// Is the caller allowed to mutate central settings? Admins always are; when
/// auth is disabled (single-user appliance) everyone is.
fn can_admin(state: &Mutex<Backend>, auth_user: Option<&User>) -> bool {
    let auth_enabled = state.lock().map(|b| b.auth.is_some()).unwrap_or(false);
    if !auth_enabled {
        return true;
    }
    matches!(auth_user, Some(u) if u.role == UserRole::Admin)
}

/// `GET /api/settings` — central defaults, onboarding state, and which
/// providers have a key configured (never the key values). Drives both the
/// onboarding gate and the profile/config panes.
pub fn get_settings(state: &Mutex<Backend>, auth_user: Option<&User>) -> HttpReply {
    let auth_enabled = state.lock().map(|b| b.auth.is_some()).unwrap_or(false);
    let providers: Vec<serde_json::Value> =
        LLM_PROVIDERS.iter().map(|id| serde_json::json!({ "id": id, "configured": global::has_api_key(id) })).collect();
    HttpReply::ok(&serde_json::json!({
        "default_provider": global::get_setting(DEFAULT_PROVIDER),
        "default_model": global::get_setting(DEFAULT_MODEL),
        "onboarding_completed": global::get_setting(ONBOARDING_DONE).as_deref() == Some("true"),
        "is_admin": can_admin(state, auth_user),
        "auth_enabled": auth_enabled,
        "providers": providers,
        // Org-wide allowlist of "provider:model" ids. Empty ⇒ all allowed.
        // Returned to every authenticated user so non-admin pickers can filter.
        "allowed_models": allowed_models(),
    }))
}

/// `POST /api/settings` — admin: update new-agent defaults and/or the
/// onboarding flag. Body fields are all optional; absent fields are untouched.
pub fn update_settings(state: &Mutex<Backend>, body: &[u8], auth_user: Option<&User>) -> HttpReply {
    if !can_admin(state, auth_user) {
        return HttpReply::error(403, "admin access required");
    }
    #[derive(serde::Deserialize)]
    struct Req {
        default_provider: Option<String>,
        default_model: Option<String>,
        onboarding_completed: Option<bool>,
        allowed_models: Option<Vec<String>>,
    }
    let Ok(req) = serde_json::from_slice::<Req>(body) else {
        return HttpReply::error(400, "malformed settings body");
    };
    if let Some(models) = req.allowed_models {
        let json = serde_json::to_string(&models).unwrap_or_else(|_| "[]".to_owned());
        if let Err(e) = global::set_setting(ALLOWED_MODELS, &json) {
            return HttpReply::error(500, &e);
        }
    }
    if let Some(p) = req.default_provider.as_deref()
        && let Err(e) = global::set_setting(DEFAULT_PROVIDER, p)
    {
        return HttpReply::error(500, &e);
    }
    if let Some(m) = req.default_model.as_deref()
        && let Err(e) = global::set_setting(DEFAULT_MODEL, m)
    {
        return HttpReply::error(500, &e);
    }
    if let Some(done) = req.onboarding_completed
        && let Err(e) = global::set_setting(ONBOARDING_DONE, if done { "true" } else { "" })
    {
        return HttpReply::error(500, &e);
    }
    get_settings(state, auth_user)
}
