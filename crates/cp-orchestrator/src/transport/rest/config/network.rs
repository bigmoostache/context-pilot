//! Network REST handlers — the uplink + access-point surface (design
//! `docs/design-network-uplink.md` §10), gated on `can_manage_it` (admin+).
//!
//! Thin wrappers over [`network`](crate::transport::it::network), in the same
//! shape as the identity handlers next door: enforce the capability, then
//! delegate. Gate semantics mirror the rest of the RBAC surface — a `None`
//! `auth_user` means access control is off (god-mode, FR-v3-08) and passes
//! through; a present caller without `can_manage_it` is a `403`. Client gating
//! is cosmetic, the server is authoritative (NFR-NET-03).

use std::sync::Mutex;

use super::super::{Backend, HttpReply};
use crate::services::auth::types::User;
use crate::transport::it::network;

/// The one gate every route below shares. Returns the `403` reply to send, or
/// `None` when the caller may proceed.
fn denied(auth_user: Option<&User>) -> Option<HttpReply> {
    if auth_user.is_some_and(|user| !user.can_manage_it()) {
        return Some(HttpReply::error(403, "IT management access required"));
    }
    None
}

/// `GET /api/it/network` — the configuration (secrets elided) plus live status.
pub(crate) fn it_get_network(state: &Mutex<Backend>, auth_user: Option<&User>) -> HttpReply {
    denied(auth_user).unwrap_or_else(|| network::get_network(state))
}

/// `POST /api/it/network/mode` — select `wan`, `wan_5g` or `5g` (FR-NET-03).
pub(crate) fn it_set_network_mode(state: &Mutex<Backend>, body: &[u8], auth_user: Option<&User>) -> HttpReply {
    denied(auth_user).unwrap_or_else(|| network::set_mode(state, body))
}

/// `POST /api/it/network/ap` — the access-point settings (FR-NET-07/08/09).
/// A `400` when the body is invalid, and specifically when the AP is enabled
/// without a regulatory country (FR-NET-14).
pub(crate) fn it_set_network_ap(state: &Mutex<Backend>, body: &[u8], auth_user: Option<&User>) -> HttpReply {
    denied(auth_user).unwrap_or_else(|| network::set_ap(state, body))
}

/// `POST /api/it/network/wwan` — the 5G bearer settings (FR-NET-15).
pub(crate) fn it_set_network_wwan(state: &Mutex<Backend>, body: &[u8], auth_user: Option<&User>) -> HttpReply {
    denied(auth_user).unwrap_or_else(|| network::set_wwan(state, body))
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
            Duration::from_secs(3600),
        );
        std::mem::forget(dir);
        Mutex::new(backend)
    }

    /// Every `/api/it/network*` handler is gated on `can_manage_it`:
    /// `manager`/`user` → 403; `admin`/`superadmin` → the delegate's own status.
    #[test]
    fn network_gated() {
        let state = backend();
        let mode = br#"{"mode":"wan"}"#;
        let access_point = br#"{"enabled":false,"ssid":"x","band":"a","channel":0,"country":"FR","hidden":false,"share_internet":true}"#;
        let wwan = br#"{"apn":"orange.fr","roaming":false,"standby":"hot"}"#;
        for role in [Manager, Regular] {
            let caller = user(role);
            assert_eq!(it_get_network(&state, Some(&caller)).status, 403, "GET denied for {role:?}");
            assert_eq!(it_set_network_mode(&state, mode, Some(&caller)).status, 403, "mode denied for {role:?}");
            assert_eq!(it_set_network_ap(&state, access_point, Some(&caller)).status, 403, "ap denied for {role:?}");
            assert_eq!(it_set_network_wwan(&state, wwan, Some(&caller)).status, 403, "wwan denied for {role:?}");
        }
        for role in [Admin, Superadmin] {
            let caller = user(role);
            assert_eq!(it_get_network(&state, Some(&caller)).status, 200, "GET ok for {role:?}");
            assert_eq!(it_set_network_mode(&state, mode, Some(&caller)).status, 200, "mode ok for {role:?}");
            assert_eq!(it_set_network_ap(&state, access_point, Some(&caller)).status, 200, "ap ok for {role:?}");
            assert_eq!(it_set_network_wwan(&state, wwan, Some(&caller)).status, 200, "wwan ok for {role:?}");
        }
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
        assert_eq!(it_set_network_mode(&state, br#"{"mode":"satellite"}"#, None).status, 400, "unknown mode → 400");
        let no_country = br#"{"enabled":true,"ssid":"cp","passphrase":"abcdefghij","band":"a","channel":0,"country":"","hidden":false,"share_internet":true}"#;
        assert_eq!(it_set_network_ap(&state, no_country, None).status, 400, "enabling with no country → 400");
    }

    /// FR-NET-13 at the transport boundary: a PSK that was just written is not
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
