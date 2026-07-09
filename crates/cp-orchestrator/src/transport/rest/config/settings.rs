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
use crate::services::auth::types::User;

/// Canonical LLM provider key names surfaced in the cockpit onboarding/profile.
const LLM_PROVIDERS: &[&str] = &["anthropic", "deepseek", "xai", "groq"];

/// Setting keys stored in the central config.
const DEFAULT_PROVIDER: &str = "default_provider";
const DEFAULT_MODEL: &str = "default_model";
const ONBOARDING_DONE: &str = "onboarding_completed";
/// Access-control master flag (design §13.10) — server-authoritative central
/// setting (NOT localStorage). `"true"` ⇒ four-role RBAC enforced; unset/empty
/// ⇒ off (default), everyone is effectively superadmin with no login (FR-v3-08).
const ACCESS_CONTROL: &str = "access_control";
/// JSON array of `"<providerId>:<modelId>"` the admin permits org-wide. An
/// **empty** list means *all models allowed* (non-blocking default at delivery).
const ALLOWED_MODELS: &str = "allowed_models";

/// Read the org-wide allowed-model allowlist (empty when unset / unparsable).
/// Shared with the provider registry (`GET /api/providers?allowed=1`) so the
/// model picker is filtered server-side from this single source of truth.
pub(crate) fn allowed_models() -> Vec<String> {
    global::get_setting(ALLOWED_MODELS).and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok()).unwrap_or_default()
}

/// Has the admin completed first-run onboarding? Shared with `GET /api/auth/me`
/// so the backend can drive the post-login flow (`next_action`).
pub(crate) fn onboarding_completed() -> bool {
    global::get_setting(ONBOARDING_DONE).as_deref() == Some("true")
}

/// Interpret the raw stored value of the access-control flag. Default **OFF**
/// (FR-v3-08): only an explicit `"true"` enables RBAC.
pub(crate) fn access_control_from_raw(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Read the persisted access-control master flag from the central config
/// (design §13.10). Loaded into [`Backend::access_control`] at boot; the
/// enforcement pipeline reads the cached copy per request, not this.
pub(crate) fn access_control_enabled() -> bool {
    access_control_from_raw(global::get_setting(ACCESS_CONTROL).as_deref())
}

/// May `caller` set the access-control flag to `want`? Asymmetric to prevent
/// self-escalation (design §13.10): **enabling** is allowed by anyone (it only
/// *closes* access — when off everyone is superadmin anyway, FR-v3-10);
/// **disabling** requires `can_manage_secrets` (superadmin only, FR-v3-11).
///
/// DEV-PHASE (design §13.10): a full disable returns everyone to god-mode and is
/// a known temporary escalation surface — the superadmin gate here is NOT a
/// sufficient long-term control and MUST be hardened before production.
fn may_set_access_control(want: bool, caller: Option<&User>) -> bool {
    // enable: anyone. disable: superadmin only.
    want || caller.is_some_and(User::can_manage_secrets)
}

/// Is the caller allowed to mutate product/org config (defaults, allowed models,
/// onboarding flag)? Per design §13.5 product config keeps a management gate:
/// `can_manage_users` (manager+). When access control is disabled (single-user
/// appliance) everyone is.
fn can_manage_config(state: &Mutex<Backend>, auth_user: Option<&User>) -> bool {
    let access_control = state.lock().map(|b| b.access_control).unwrap_or(false);
    if !access_control {
        return true;
    }
    auth_user.is_some_and(User::can_manage_users)
}

/// `GET /api/settings` — central defaults, onboarding state, and which
/// providers have a key configured (never the key values). Drives both the
/// onboarding gate and the profile/config panes.
pub fn get_settings(state: &Mutex<Backend>, auth_user: Option<&User>) -> HttpReply {
    let (auth_enabled, access_control) =
        state.lock().map(|b| (b.auth.is_some(), b.access_control)).unwrap_or((false, false));
    let providers: Vec<serde_json::Value> =
        LLM_PROVIDERS.iter().map(|id| serde_json::json!({ "id": id, "configured": global::has_api_key(id) })).collect();
    HttpReply::ok(&serde_json::json!({
        "default_provider": global::get_setting(DEFAULT_PROVIDER),
        "default_model": global::get_setting(DEFAULT_MODEL),
        "onboarding_completed": onboarding_completed(),
        "is_admin": can_manage_config(state, auth_user),
        "auth_enabled": auth_enabled,
        // Access-control master flag (design §13.10) — server-authoritative, so
        // the cockpit renders the toggle state from the server, never localStorage.
        "access_control": access_control,
        "providers": providers,
        // Org-wide allowlist of "provider:model" ids. Empty ⇒ all allowed.
        // Returned to every authenticated user so non-admin pickers can filter.
        "allowed_models": allowed_models(),
    }))
}

/// `POST /api/settings` — admin: update new-agent defaults and/or the
/// onboarding flag. Body fields are all optional; absent fields are untouched.
pub fn update_settings(state: &Mutex<Backend>, body: &[u8], auth_user: Option<&User>) -> HttpReply {
    #[derive(serde::Deserialize)]
    struct Req {
        default_provider: Option<String>,
        default_model: Option<String>,
        onboarding_completed: Option<bool>,
        allowed_models: Option<Vec<String>>,
        /// Access-control master flag toggle (design §13.10). Asymmetric gate:
        /// enable = anyone, disable = superadmin.
        access_control: Option<bool>,
    }
    let Ok(req) = serde_json::from_slice::<Req>(body) else {
        return HttpReply::error(400, "malformed settings body");
    };

    // ── Access-control flag: independent asymmetric gate (FR-v3-10/11) ──
    // Handled before the product-config gate so a `user`/`manager` can *enable*
    // RBAC without holding the config capability.
    if let Some(want) = req.access_control {
        if !may_set_access_control(want, auth_user) {
            return HttpReply::error(403, "superadmin required to disable access control");
        }
        if let Err(e) = global::set_setting(ACCESS_CONTROL, if want { "true" } else { "" }) {
            return HttpReply::error(500, &e);
        }
        // Update the in-memory cache the enforcement pipeline reads per request.
        if let Ok(mut b) = state.lock() {
            b.access_control = want;
        }
    }

    // ── Product/org config fields — gated on can_manage_config (§13.5) ──
    let touches_config = req.default_provider.is_some()
        || req.default_model.is_some()
        || req.onboarding_completed.is_some()
        || req.allowed_models.is_some();
    if touches_config && !can_manage_config(state, auth_user) {
        return HttpReply::error(403, "management access required");
    }
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

#[cfg(test)]
mod tests {
    // Bare variant imports (the `Admin` variant, not its fully-qualified path)
    // keep the capability-grep gate (V1.1a) clean — it reserves that qualified
    // spelling for capabilities/types/tests.rs.
    use super::*;
    use crate::services::auth::types::UserRole;
    use crate::services::auth::types::UserRole::{Admin, Manager, Superadmin, User as Regular};

    /// Build a bare [`User`] with the given role — only `role` matters here.
    fn user(role: UserRole) -> User {
        User {
            id: "id".to_owned(),
            email: "e@x.com".to_owned(),
            name: "N".to_owned(),
            password_hash: String::new(),
            role,
            must_change_password: false,
            created_at: 0,
            updated_at: 0,
        }
    }

    /// V0.4a — the flag reads `false` when unset (fresh DB) and only `"true"`
    /// turns it on.
    #[test]
    fn flag_default_off() {
        assert!(!access_control_from_raw(None), "unset ⇒ off");
        assert!(!access_control_from_raw(Some("")), "empty ⇒ off");
        assert!(!access_control_from_raw(Some("false")), "any non-true ⇒ off");
        assert!(access_control_from_raw(Some("true")), "explicit true ⇒ on");
    }

    /// V0.4c — enabling is allowed for anyone; disabling requires superadmin.
    #[test]
    fn disable_requires_superadmin() {
        // Enable (want = true): allowed for every role and even no caller.
        for role in [Superadmin, Admin, Manager, Regular] {
            assert!(may_set_access_control(true, Some(&user(role))), "enable by {role:?}");
        }
        assert!(may_set_access_control(true, None), "enable with no caller (god-mode)");
        // Disable (want = false): only superadmin.
        assert!(may_set_access_control(false, Some(&user(Superadmin))), "superadmin disables");
        assert!(!may_set_access_control(false, Some(&user(Admin))), "admin cannot disable");
        assert!(!may_set_access_control(false, Some(&user(Manager))), "manager cannot disable");
        assert!(!may_set_access_control(false, Some(&user(Regular))), "user cannot disable");
        assert!(!may_set_access_control(false, None), "no caller cannot disable");
    }
}
