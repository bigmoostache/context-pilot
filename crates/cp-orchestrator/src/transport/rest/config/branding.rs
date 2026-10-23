//! Cockpit branding — the browser tab title and favicon, set once for every
//! browser by an admin from the settings page.
//!
//! The title lives in the central config (`tab_title`); the favicon image in the
//! [`Backend::branding`] store under [`FAVICON_KEY`], with `favicon_version`
//! (upload time, ms) in the config so the web app can cache-bust its `<link>`.
//! Reads are public: the tab shows the branding before anyone logs in.

use std::sync::Mutex;

use cp_base::config::global;

use super::super::{Backend, HttpReply};
use super::settings::can_manage_config;
use crate::services::auth::types::User;

/// Central-config key of the custom tab title (absent or empty = default title).
const TAB_TITLE: &str = "tab_title";
/// Central-config key of the favicon upload time (ms), the cache-busting token.
const FAVICON_VERSION: &str = "favicon_version";
/// Entry name of the favicon inside [`Backend::branding`].
pub(crate) const FAVICON_KEY: &str = "favicon";
/// Longest accepted tab title, in characters.
const MAX_TITLE_CHARS: usize = 120;

/// A config value, with empty read as unset.
fn non_empty(key: &str) -> Option<String> {
    global::get_setting(key).filter(|v| !v.is_empty())
}

/// `GET /api/branding` (public): `{ title, favicon_version }`, each `null` when
/// the default applies.
pub(crate) fn get_branding(state: &Mutex<Backend>) -> HttpReply {
    let has_favicon = state.lock().is_ok_and(|b| b.branding.has(FAVICON_KEY));
    HttpReply::ok(&serde_json::json!({
        "title": non_empty(TAB_TITLE),
        "favicon_version": if has_favicon { non_empty(FAVICON_VERSION) } else { None },
    }))
}

/// `POST /api/settings/branding` (admin): `{ "title": "…" }`; an empty or
/// blank title restores the default.
pub(crate) fn update_branding(state: &Mutex<Backend>, body: &[u8], auth_user: Option<&User>) -> HttpReply {
    #[derive(serde::Deserialize)]
    struct Req {
        title: String,
    }
    if !can_manage_config(state, auth_user) {
        return HttpReply::error(403, "management access required");
    }
    let Ok(req) = serde_json::from_slice::<Req>(body) else {
        return HttpReply::error(400, "malformed branding body");
    };
    let title = req.title.trim();
    if title.chars().count() > MAX_TITLE_CHARS {
        return HttpReply::error(400, "title too long");
    }
    if let Err(e) = global::set_setting(TAB_TITLE, title) {
        return HttpReply::error(500, &e);
    }
    get_branding(state)
}

/// `POST /api/settings/favicon` (admin): raw image bytes (PNG/JPEG/GIF/WebP/
/// BMP/ICO/SVG, sniffed, max 2 MiB) replace the favicon.
pub(crate) fn upload_favicon(state: &Mutex<Backend>, body: &[u8], auth_user: Option<&User>) -> HttpReply {
    if !can_manage_config(state, auth_user) {
        return HttpReply::error(403, "management access required");
    }
    if body.is_empty() {
        return HttpReply::error(400, "empty body");
    }
    let stored = state
        .lock()
        .map_err(|_poison| "backend lock poisoned".to_owned())
        .and_then(|mut b| b.branding.set(FAVICON_KEY, body));
    if let Err(msg) = stored {
        return HttpReply::error(400, &msg);
    }
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    if let Err(e) = global::set_setting(FAVICON_VERSION, &now_ms.to_string()) {
        return HttpReply::error(500, &e);
    }
    get_branding(state)
}

/// `DELETE /api/settings/favicon` (admin): back to the built-in favicon.
pub(crate) fn delete_favicon(state: &Mutex<Backend>, auth_user: Option<&User>) -> HttpReply {
    if !can_manage_config(state, auth_user) {
        return HttpReply::error(403, "management access required");
    }
    if let Ok(mut b) = state.lock() {
        let _existed = b.branding.remove(FAVICON_KEY);
    }
    get_branding(state)
}
