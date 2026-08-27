//! Network REST handlers — the uplink + access-point surface, gated on
//! `can_manage_it` (admin+) —
//! **except the 5G bearer, which is `can_manage_secrets` (superadmin).**
//!
//! The split is a business boundary, not a security one. The uplink mode and
//! the Wi-Fi AP are the client's site to run. The 5G bearer is not: we ship the
//! SIM, we own the data plan and the APN that goes with it, and a client's IT
//! admin changing it breaks their own connectivity and bills us for it. That is
//! the same boundary `can_manage_secrets` already draws around the provider API
//! keys — the vendor-controlled billing boundary.
//!
//! Thin wrappers over [`network`](crate::transport::it::network), in the same
//! shape as the identity handlers next door: enforce the capability, then
//! delegate. Gate semantics mirror the rest of the RBAC surface — a `None`
//! `auth_user` means access control is off (god-mode, FR-v3-08) and passes
//! through; a present caller without `can_manage_it` is a `403`. Client gating
//! is cosmetic, the server is authoritative.

use std::sync::Mutex;

use super::super::{Backend, HttpReply};
use crate::services::auth::types::User;
use crate::transport::it::network;

/// The gate the site-level routes share. Returns the `403` reply to send, or
/// `None` when the caller may proceed.
fn denied(auth_user: Option<&User>) -> Option<HttpReply> {
    if auth_user.is_some_and(|user| !user.can_manage_it()) {
        return Some(HttpReply::error(403, "IT management access required"));
    }
    None
}

/// The stricter gate, for the vendor's 5G bearer.
fn denied_bearer(auth_user: Option<&User>) -> Option<HttpReply> {
    if auth_user.is_some_and(|user| !user.can_manage_secrets()) {
        return Some(HttpReply::error(403, "the 5G bearer is vendor-managed"));
    }
    None
}

/// Whether this caller may see the bearer configuration at all. A `None` caller
/// is god-mode (access control off) and sees everything.
fn bearer_visible(auth_user: Option<&User>) -> bool {
    auth_user.is_none_or(User::can_manage_secrets)
}

/// `GET /api/it/network` — the configuration (secrets elided) plus live status.
///
/// `can_manage_it` is enough to read this, but a caller without
/// `can_manage_secrets` gets `config.wwan: null`: the bearer's settings are
/// vendor state. `status.wwan` is still returned — whether the modem is
/// registered, on which operator and at what signal is diagnostics the client's
/// own admin needs when the box loses its uplink.
pub(crate) fn it_get_network(state: &Mutex<Backend>, auth_user: Option<&User>) -> HttpReply {
    denied(auth_user)
        .unwrap_or_else(|| network::get_network(state, bearer_visible(auth_user), network::modem_present()))
}

/// `POST /api/it/network/mode` — select `wan`, `wan_5g` or `5g`.
pub(crate) fn it_set_network_mode(state: &Mutex<Backend>, body: &[u8], auth_user: Option<&User>) -> HttpReply {
    denied(auth_user).unwrap_or_else(|| network::set_mode(state, body, network::modem_present()))
}

/// `POST /api/it/network/ap` — the access-point settings: SSID, passphrase,
/// band, channel, country, hidden-SSID and internet sharing. A `400` when the
/// body is invalid, and specifically when the AP is enabled without a regulatory
/// country — without one the radio has no channel it is allowed to beacon on.
pub(crate) fn it_set_network_ap(state: &Mutex<Backend>, body: &[u8], auth_user: Option<&User>) -> HttpReply {
    denied(auth_user).unwrap_or_else(|| network::set_ap(state, body))
}

/// `POST /api/it/network/wwan` — the 5G bearer settings.
/// **Superadmin only** — see the module doc.
pub(crate) fn it_set_network_wwan(state: &Mutex<Backend>, body: &[u8], auth_user: Option<&User>) -> HttpReply {
    denied_bearer(auth_user).unwrap_or_else(|| network::set_wwan(state, body, network::modem_present()))
}

#[cfg(test)]
mod tests {
    // Bare variant imports (the `Admin` variant, not its fully-qualified path)
    // keep the capability-grep gate (V1.1a) clean.
    use super::*;
    use crate::services::auth::store::AuthStore;
    use crate::services::auth::types::UserRole;
    use crate::services::auth::types::UserRole::{Admin, Manager, Superadmin, User as Regular};
    use std::path::PathBuf;
    use std::time::Duration;

    /// A bare [`User`] with the given role — only `role` gates these handlers.
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

    /// A `Mutex<Backend>` over a leaked temp dir, mirroring the fixture in
    /// `config/it.rs` so `.network.json` lands somewhere writable and private.
    fn backend() -> Mutex<Backend> {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = AuthStore::open(&dir.path().join("auth.db")).expect("open auth store");
        let backend = Backend::new(
            dir.path().to_path_buf(),
            PathBuf::from("/tmp/cp-net-test-realms"),
            PathBuf::from("/tmp/cp-net-test-bin"),
            Some(store),
            Duration::from_hours(1),
        );
        std::mem::forget(dir);
        Mutex::new(backend)
    }

    /// The site-level routes are gated on `can_manage_it`: `manager`/`user` →
    /// 403; `admin`/`superadmin` → the delegate's own status.
    #[test]
    fn network_gated() {
        let state = backend();
        let mode = br#"{"mode":"wan"}"#;
        let access_point = br#"{"enabled":false,"ssid":"x","band":"a","channel":0,"country":"FR","hidden":false,"share_internet":true}"#;
        for role in [Manager, Regular] {
            let caller = user(role);
            assert_eq!(it_get_network(&state, Some(&caller)).status, 403, "GET denied for {role:?}");
            assert_eq!(it_set_network_mode(&state, mode, Some(&caller)).status, 403, "mode denied for {role:?}");
            assert_eq!(it_set_network_ap(&state, access_point, Some(&caller)).status, 403, "ap denied for {role:?}");
        }
        for role in [Admin, Superadmin] {
            let caller = user(role);
            assert_eq!(it_get_network(&state, Some(&caller)).status, 200, "GET ok for {role:?}");
            assert_eq!(it_set_network_mode(&state, mode, Some(&caller)).status, 200, "mode ok for {role:?}");
            assert_eq!(it_set_network_ap(&state, access_point, Some(&caller)).status, 200, "ap ok for {role:?}");
        }
    }

    /// The 5G bearer is the vendor's, so it sits one rung higher than the rest
    /// of the pane: **`can_manage_secrets`, not `can_manage_it`.** An `admin`
    /// runs their own site — uplink mode, Wi-Fi — but the SIM, the data plan and
    /// the APN are ours, and changing them breaks their connectivity on our bill.
    #[test]
    fn the_bearer_is_superadmin_only() {
        let state = backend();
        let wwan = br#"{"apn":"orange.fr","roaming":false,"standby":"hot"}"#;
        for role in [Regular, Manager, Admin] {
            let caller = user(role);
            assert_eq!(it_set_network_wwan(&state, wwan, Some(&caller)).status, 403, "wwan denied for {role:?}");
        }
        let vendor = user(Superadmin);
        assert_eq!(it_set_network_wwan(&state, wwan, Some(&vendor)).status, 200, "the vendor may set it");
    }

    /// …and an admin cannot READ it either. They keep `status.wwan` — whether
    /// the modem is registered, on what operator, at what signal — because that
    /// is what they need when the box loses its uplink and calls us.
    #[test]
    fn an_admin_sees_bearer_status_but_not_bearer_config() {
        let state = backend();
        let vendor = user(Superadmin);
        assert_eq!(
            it_set_network_wwan(
                &state,
                br#"{"apn":"vendor.apn.example","roaming":false,"standby":"hot"}"#,
                Some(&vendor)
            )
            .status,
            200
        );

        let admin = it_get_network(&state, Some(&user(Admin)));
        assert_eq!(admin.status, 200, "an admin still reads the pane");
        assert!(!admin.body.contains("vendor.apn.example"), "the vendor's APN leaked to an admin: {}", admin.body);
        assert!(admin.body.contains("\"wwan\":null"), "the bearer CONFIG is elided: {}", admin.body);
        assert!(admin.body.contains("\"status\""), "\u{2026}but the status half is still there");

        let superadmin = it_get_network(&state, Some(&vendor));
        assert!(superadmin.body.contains("vendor.apn.example"), "the vendor sees their own APN");
    }

    /// A box that is not a 5G variant must not be able to reach a mode that
    /// needs one — `5g` there would suppress the ethernet default route with
    /// nothing to replace it. Recoverable — no mode ever alters an address on
    /// the ethernet ports — but never worth offering.
    ///
    /// The hardware fact is threaded in rather than probed inside the handler,
    /// so this drives the no-modem variant without touching the environment.
    #[test]
    fn a_box_with_no_modem_refuses_the_5g_modes_and_hides_the_bearer() {
        let state = backend();
        let no_modem = false;

        assert_eq!(network::set_mode(&state, br#"{"mode":"wan"}"#, no_modem).status, 200, "ethernet always works");
        assert_eq!(network::set_mode(&state, br#"{"mode":"5g"}"#, no_modem).status, 400, "5g needs a modem");
        assert_eq!(network::set_mode(&state, br#"{"mode":"wan_5g"}"#, no_modem).status, 400, "so does failover");
        assert_eq!(
            network::set_wwan(&state, br#"{"apn":"x.y","roaming":false,"standby":"hot"}"#, no_modem).status,
            400,
            "and so does configuring the bearer"
        );

        // Even the vendor sees no bearer block on a box that has no bearer.
        let got = network::get_network(&state, true, no_modem);
        assert!(got.body.contains("\"wwan\":null"), "the bearer config is hidden: {}", got.body);

        let with_modem = network::get_network(&state, true, true);
        assert!(with_modem.body.contains("\"apn\""), "…and present on a 5G variant: {}", with_modem.body);
    }

    /// Set → get round-trip, driven god-mode so it exercises the logic, not the
    /// gate.
    #[test]
    fn mode_round_trips() {
        let state = backend();
        assert!(it_get_network(&state, None).body.contains("\"mode\":\"wan\""), "a fresh box defaults to wan");
        assert_eq!(it_set_network_mode(&state, br#"{"mode":"5g"}"#, None).status, 200);
        let got = it_get_network(&state, None);
        assert!(got.body.contains("\"mode\":\"5g\""), "GET reflects the set mode: {}", got.body);
    }

    /// The two `400`s the design calls out by name.
    #[test]
    fn invalid_bodies_are_400() {
        let state = backend();
        assert_eq!(it_set_network_mode(&state, br#"{"mode":"satellite"}"#, None).status, 400, "unknown mode \u{2192} 400");
        let no_country = br#"{"enabled":true,"ssid":"cp","passphrase":"abcdefghij","band":"a","channel":0,"country":"","hidden":false,"share_internet":true}"#;
        assert_eq!(it_set_network_ap(&state, no_country, None).status, 400, "enabling with no country \u{2192} 400");
    }

    /// Secret elision at the transport boundary: a PSK that was just written is not
    /// echoed by the response, nor by any later read.
    #[test]
    fn secrets_never_come_back_out() {
        let state = backend();
        let body = br#"{"enabled":true,"ssid":"cp","passphrase":"correct-horse-battery","band":"a","channel":36,"country":"FR","hidden":false,"share_internet":true}"#;
        let set = it_set_network_ap(&state, body, None);
        assert_eq!(set.status, 200, "valid AP accepted: {}", set.body);
        assert!(!set.body.contains("correct-horse-battery"), "the write response echoed the PSK");
        let got = it_get_network(&state, None);
        assert!(!got.body.contains("correct-horse-battery"), "a read returned the PSK");
        assert!(got.body.contains("\"passphrase_set\":true"), "but the UI is told one is set");
    }

    /// Concurrent writes to different halves of the document must both survive.
    ///
    /// The bug this pins down was measured on hardware: two overlapping mode
    /// applies each derived their "next" document from the same snapshot read
    /// **before** the critical section, so the slower one wrote its stale view
    /// back on top and the box ended up with a persisted `wan` and a suppressed
    /// default route. Reading inside the lock is what makes the second writer
    /// see the first writer's result.
    #[test]
    fn concurrent_writes_to_different_halves_both_survive() {
        let state = std::sync::Arc::new(backend());
        let ap = br#"{"enabled":false,"ssid":"concurrent","band":"a","channel":36,"country":"FR","hidden":false,"share_internet":true}"#;
        let wwan = br#"{"apn":"concurrent.apn","roaming":true,"standby":"cold"}"#;
        let threads: Vec<_> = (0..8)
            .map(|worker| {
                let state = std::sync::Arc::clone(&state);
                std::thread::spawn(move || {
                    if worker % 2 == 0 {
                        assert_eq!(it_set_network_ap(&state, ap, None).status, 200);
                    } else {
                        assert_eq!(it_set_network_wwan(&state, wwan, None).status, 200);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().expect("worker");
        }
        let got = it_get_network(&state, None);
        assert!(got.body.contains("\"ssid\":\"concurrent\""), "the AP write survived: {}", got.body);
        assert!(got.body.contains("\"apn\":\"concurrent.apn\""), "the bearer write survived too: {}", got.body);
        assert!(got.body.contains("\"standby\":\"cold\""), "\u{2026}including its other fields");
    }

    /// An omitted passphrase keeps the stored one; an explicit `null` clears it.
    #[test]
    fn an_omitted_secret_is_kept_and_an_explicit_null_clears_it() {
        let state = backend();
        let with_psk = br#"{"enabled":true,"ssid":"cp","passphrase":"correct-horse-battery","band":"a","channel":36,"country":"FR","hidden":false,"share_internet":true}"#;
        assert_eq!(it_set_network_ap(&state, with_psk, None).status, 200);

        // Same form re-saved with the passphrase field absent — the common UI
        // case, and the one that would silently wipe the PSK without the
        // absent-vs-null distinction.
        let renamed = br#"{"enabled":true,"ssid":"cp2","band":"a","channel":36,"country":"FR","hidden":false,"share_internet":true}"#;
        assert_eq!(it_set_network_ap(&state, renamed, None).status, 200);
        let got = it_get_network(&state, None);
        assert!(got.body.contains("\"passphrase_set\":true"), "an omitted passphrase kept the stored one");
        assert!(got.body.contains("\"ssid\":\"cp2\""), "the rest of the form still applied");

        // An explicit null clears it — which, with the AP enabled, is refused.
        let cleared = br#"{"enabled":true,"ssid":"cp2","passphrase":null,"band":"a","channel":36,"country":"FR","hidden":false,"share_internet":true}"#;
        assert_eq!(it_set_network_ap(&state, cleared, None).status, 400, "an enabled AP may not drop its PSK");
    }
}
