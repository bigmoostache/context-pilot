//! *Update* REST handlers (O5.1) — the cockpit face of the auto-updater.
//!
//! Four routes, all gated on `can_manage_it` by the dispatch guard in
//! [`route_rest`](crate::transport) (a `None` caller is god-mode, §13.10):
//!
//! * `GET  /api/update/status` — version/channel/mode/window + last check &
//!   result (the durable [`UpdateState`]).
//! * `POST /api/update/check`  — force a channel poll right now.
//! * `POST /api/update/apply`  — immediate apply, ignoring the window (the
//!   admin *is* the window); refuses a second in-flight apply.
//! * `PUT  /api/update/mode`   — switch `auto`/`manual`/`paused` and/or move
//!   the maintenance window.
//!
//! The legacy arbitrary version-choice routes (`download`/`select`/`delete`)
//! are retired behind `CP_RELEASES_BREAK_GLASS` (T5.1.5) — this pane replaces
//! them.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;

use super::super::{Backend, HttpReply};
use crate::services::ReleaseStore;
use crate::services::releases::updater::{
    UpdateEvaluation, UpdateState, check_channel, download_artifact, restart_self, scheduler, stage_apply,
};
use crate::services::releases::{MaintenanceWindow, UpdateMode};

/// Process-wide apply serialisation (T4.2.3): the REST `apply` route and the
/// nightly scheduler share this gate, so no two applies ever run at once. A
/// successful apply keeps it held — the process is about to re-exec.
pub(crate) static APPLY_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// The status document every handler returns (and the pane polls).
fn status_json(b: &Backend) -> serde_json::Value {
    let st = UpdateState::load(b.releases.dir());
    let window = b.releases.window();
    serde_json::json!({
        "current": scheduler::current_version(&b.releases),
        "active_tag": b.releases.active_tag(),
        "channel": b.releases.channel(),
        "arch": b.releases.arch(),
        "mode": b.releases.update_mode().as_str(),
        "window": { "start": window.start, "end": window.end },
        "poll_interval_hours": b.releases.poll_interval_hours(),
        "available": st.available,
        "notes_url": st.available_notes_url,
        "last_check_ms": st.last_check_ms,
        "last_result": st.last_result,
        "apply_in_flight": APPLY_IN_FLIGHT.load(Ordering::SeqCst),
    })
}

/// `GET /api/update/status` — current version, channel, mode, window, last
/// check and last apply result.
pub(crate) fn update_status(state: &Mutex<Backend>) -> HttpReply {
    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    HttpReply::ok(&status_json(&b))
}

/// `POST /api/update/check` — poll the channel now, then return the
/// refreshed status. A failed check (network, signature…) is a `502` and the
/// last-known state is kept.
pub(crate) fn update_check(state: &Mutex<Backend>) -> HttpReply {
    let (releases_dir, channel, current, crossgrade) = {
        let Ok(b) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        (
            b.releases.dir().to_path_buf(),
            b.releases.channel().to_owned(),
            scheduler::current_version(&b.releases),
            b.releases.pending_channel_switch(),
        )
    };
    // Network I/O with the lock released.
    let outcome = check_channel(&releases_dir, &channel, &current, crossgrade);
    let Ok(mut b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    match outcome {
        // A verified answer on the new channel retires the crossgrade window.
        Ok(_evaluation) => {
            b.releases.clear_pending_switch();
            HttpReply::ok(&status_json(&b))
        }
        Err(e) => HttpReply::error(502, &format!("check failed: {e}")),
    }
}

/// `POST /api/update/apply` — verify, download and apply the channel version
/// immediately (off-window: an explicit admin action is its own window),
/// then restart. `409` when an apply is already in flight, `200
/// status=up_to_date` when there is nothing to do.
pub(crate) fn update_apply(state: &Mutex<Backend>) -> HttpReply {
    let Ok(install) = std::env::current_exe() else {
        return HttpReply::error(500, "cannot resolve the running binary path");
    };
    let (releases_dir, arch, channel, current, crossgrade) = {
        let Ok(b) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        (
            b.releases.dir().to_path_buf(),
            b.releases.arch().to_owned(),
            b.releases.channel().to_owned(),
            scheduler::current_version(&b.releases),
            b.releases.pending_channel_switch(),
        )
    };

    // 1. Fresh verified check (network, no lock).
    let manifest = match check_channel(&releases_dir, &channel, &current, crossgrade) {
        Err(e) => return HttpReply::error(502, &format!("check failed: {e}")),
        Ok(UpdateEvaluation::UpToDate) => {
            if let Ok(mut b) = state.lock() {
                b.releases.clear_pending_switch();
            }
            return HttpReply::ok(&serde_json::json!({ "status": "up_to_date", "current": current }));
        }
        Ok(UpdateEvaluation::Available(manifest)) => manifest,
    };
    // The switch resolved to a concrete target; retire the crossgrade window
    // now so a mid-apply failure doesn't re-trigger head-tracking next poll.
    if let Ok(mut b) = state.lock() {
        b.releases.clear_pending_switch();
    }

    // 2. Serialise applies across REST + scheduler.
    if APPLY_IN_FLIGHT.swap(true, Ordering::SeqCst) {
        return HttpReply::error(409, "an update apply is already in flight");
    }

    // 3. Download + sha verification (network, no lock).
    let snapshot = ReleaseStore::load(releases_dir);
    if let Err(e) = download_artifact(&snapshot, &manifest, &arch) {
        APPLY_IN_FLIGHT.store(false, Ordering::SeqCst);
        return HttpReply::error(502, &format!("download failed: {e}"));
    }

    // 4. Stage (DB backup + atomic binary swap) under the lock, then re-exec.
    {
        let Ok(b) = state.lock() else {
            APPLY_IN_FLIGHT.store(false, Ordering::SeqCst);
            return HttpReply::error(500, "backend lock poisoned");
        };
        if let Err(e) = stage_apply(&b.releases, b.auth.as_ref(), &b.auth_db_path, &install, &manifest.version) {
            drop(b);
            APPLY_IN_FLIGHT.store(false, Ordering::SeqCst);
            return HttpReply::error(500, &format!("stage failed: {e}"));
        }
    }
    eprintln!("updater: apply {current} → {} (admin request) — restarting", manifest.version);
    restart_self(&install);
    HttpReply::ok(&serde_json::json!({ "status": "applying", "from": current, "to": manifest.version }))
}

/// `PUT /api/update/mode` — set the update mode, channel, and/or the
/// maintenance window. Body: `{ "mode": "auto"|"manual"|"paused", "channel":
/// "stable"|"nightly", "window": { "start": "HH:MM", "end": "HH:MM" } }` (each
/// field optional, at least one required).
pub(crate) fn update_set_mode(state: &Mutex<Backend>, body: &[u8]) -> HttpReply {
    /// The publishable channels — an unknown value yields a free `400` (like
    /// `UpdateMode`), so the store never sees an invalid channel string.
    #[derive(Deserialize)]
    #[serde(rename_all = "lowercase")]
    enum Channel {
        Stable,
        Nightly,
    }
    impl Channel {
        fn as_str(&self) -> &'static str {
            match self {
                Self::Stable => "stable",
                Self::Nightly => "nightly",
            }
        }
    }
    #[derive(Deserialize)]
    struct Req {
        mode: Option<UpdateMode>,
        channel: Option<Channel>,
        window: Option<MaintenanceWindow>,
    }
    let Ok(req) = serde_json::from_slice::<Req>(body) else {
        return HttpReply::error(
            400,
            "expected {\"mode\":\"auto|manual|paused\",\"channel\":\"stable|nightly\",\"window\":{\"start\":..,\"end\":..}}",
        );
    };
    if req.mode.is_none() && req.channel.is_none() && req.window.is_none() {
        return HttpReply::error(400, "nothing to update: provide mode, channel and/or window");
    }
    let Ok(mut b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    if let Some(window) = req.window {
        if let Err(e) = b.releases.set_window(window) {
            return HttpReply::error(400, &e);
        }
    }
    if let Some(channel) = req.channel {
        if let Err(e) = b.releases.set_channel(channel.as_str()) {
            return HttpReply::error(400, &e);
        }
    }
    if let Some(mode) = req.mode {
        b.releases.set_update_mode(mode);
    }
    HttpReply::ok(&status_json(&b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::MaterializedView;
    use std::path::PathBuf;

    /// A hermetic backend whose release store lives in a fresh temp dir.
    fn backend(label: &str) -> (Mutex<Backend>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("cp-update-routes-{label}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let mut b = Backend::for_test(dir.clone(), MaterializedView::new());
        b.releases = ReleaseStore::load(dir.join("releases"));
        (Mutex::new(b), dir)
    }

    /// V5.1b — `status` reports the defaults; `mode` flips the posture and
    /// moves the window (server-persisted, reflected in the next status).
    #[test]
    fn update_routes_status_and_mode_roundtrip() {
        let (state, dir) = backend("mode");

        let status = update_status(&state);
        assert_eq!(status.status, 200);
        assert!(status.body.contains("\"mode\":\"auto\""), "default mode: {}", status.body);
        assert!(status.body.contains("\"start\":\"03:00\""), "default window: {}", status.body);

        let set = update_set_mode(&state, br#"{"mode":"manual","window":{"start":"22:00","end":"23:30"}}"#);
        assert_eq!(set.status, 200, "{}", set.body);
        assert!(set.body.contains("\"mode\":\"manual\""));
        assert!(set.body.contains("\"start\":\"22:00\""));

        // Persisted server-side: a reloaded store sees the same values.
        let reloaded = ReleaseStore::load(dir.join("releases"));
        assert_eq!(reloaded.update_mode(), UpdateMode::Manual);
        assert_eq!(reloaded.window().start, "22:00");

        drop(std::fs::remove_dir_all(&dir));
    }

    /// The channel field on `PUT /api/update/mode` switches the box channel
    /// (reflected in the next status) and rejects unknown channels with `400`.
    #[test]
    fn update_routes_channel_switch() {
        let (state, dir) = backend("channel");

        assert!(update_status(&state).body.contains("\"channel\":\"stable\""), "default channel");

        let set = update_set_mode(&state, br#"{"channel":"nightly"}"#);
        assert_eq!(set.status, 200, "{}", set.body);
        assert!(set.body.contains("\"channel\":\"nightly\""), "channel switched: {}", set.body);

        // Persisted server-side.
        let reloaded = ReleaseStore::load(dir.join("releases"));
        assert_eq!(reloaded.channel(), "nightly");
        assert!(reloaded.pending_channel_switch(), "switch armed the crossgrade flag");

        // An unknown channel is refused and changes nothing.
        assert_eq!(update_set_mode(&state, br#"{"channel":"beta"}"#).status, 400, "unknown channel refused");
        assert!(update_status(&state).body.contains("\"channel\":\"nightly\""));

        drop(std::fs::remove_dir_all(&dir));
    }

    /// V5.1b — malformed bodies and windows are refused with `400`.
    #[test]
    fn update_routes_mode_validation() {
        let (state, dir) = backend("validation");
        assert_eq!(update_set_mode(&state, b"{not json").status, 400);
        assert_eq!(update_set_mode(&state, b"{}").status, 400, "empty update refused");
        assert_eq!(update_set_mode(&state, br#"{"mode":"yolo"}"#).status, 400, "unknown mode refused");
        assert_eq!(
            update_set_mode(&state, br#"{"window":{"start":"9:99","end":"05:00"}}"#).status,
            400,
            "malformed window refused"
        );
        // Nothing was persisted by the refused writes.
        assert!(update_status(&state).body.contains("\"mode\":\"auto\""));
        drop(std::fs::remove_dir_all(&dir));
    }
}
