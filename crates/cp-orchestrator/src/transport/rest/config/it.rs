//! IT-infrastructure REST handlers (design §13.3/§13.5/§13.7) — the former
//! maintenance plane's IT functions re-homed onto the product API (`:443`),
//! gated on the `can_manage_it` capability (admin+).
//!
//! These are thin wrappers over the retained IT modules
//! ([`ca`](crate::transport::it::ca),
//! [`identity`](crate::transport::it::identity),
//! [`state`](crate::transport::it::state)): each first enforces the
//! capability, then delegates to the shared implementation. The separate
//! maintenance plane was removed in the M5 teardown (design §13.4/§13.8); these
//! are the only face onto these functions now.
//!
//! Gate semantics mirror the rest of the RBAC surface: a `None` `auth_user`
//! means access control is off (god-mode, FR-v3-08) and passes through; a
//! present caller without `can_manage_it` is a `403` (client gating is cosmetic,
//! the server is authoritative — NFR-05). The raw `GET /api/it/ca.crt` download
//! needs the [`Request`](tiny_http::Request) itself and so is gated + served in
//! [`crate::transport::handle`], not here.

use std::sync::Mutex;

use super::super::{Backend, HttpReply};
use crate::services::auth::types::User;
use crate::transport::it::{ca, identity};

/// `GET /api/it/ca/fingerprint` — the private-CA root's SHA-256 fingerprint
/// (`can_manage_it`). Delegates to [`ca::ca_fingerprint`].
pub(crate) fn it_ca_fingerprint(auth_user: Option<&User>) -> HttpReply {
    if auth_user.is_some_and(|u| !u.can_manage_it()) {
        return HttpReply::error(403, "IT management access required");
    }
    ca::ca_fingerprint()
}

/// `GET /api/it/identity` — the current box name/IP, or `null` (`can_manage_it`).
/// Delegates to [`identity::get_identity`].
pub(crate) fn it_get_identity(state: &Mutex<Backend>, auth_user: Option<&User>) -> HttpReply {
    if auth_user.is_some_and(|u| !u.can_manage_it()) {
        return HttpReply::error(403, "IT management access required");
    }
    identity::get_identity(state)
}

/// `POST /api/it/identity` — set the box name/IP, re-issue the leaf, reload Caddy
/// (`can_manage_it`). Delegates to [`identity::set_identity`], which validates the
/// body (`400` on a bad name/IP), persists, and regenerates the cert.
pub(crate) fn it_set_identity(state: &Mutex<Backend>, body: &[u8], auth_user: Option<&User>) -> HttpReply {
    if auth_user.is_some_and(|u| !u.can_manage_it()) {
        return HttpReply::error(403, "IT management access required");
    }
    identity::set_identity(state, body)
}

/// `GET /api/it/provisioned` — whether the box has been provisioned
/// (`can_manage_it`). Reads the durable flag via
/// [`state::is_provisioned`](crate::transport::it::state::is_provisioned).
pub(crate) fn it_provisioned(state: &Mutex<Backend>, auth_user: Option<&User>) -> HttpReply {
    if auth_user.is_some_and(|u| !u.can_manage_it()) {
        return HttpReply::error(403, "IT management access required");
    }
    let provisioned = match state.lock() {
        Ok(b) => crate::transport::it::state::is_provisioned(&b.provision_flag_path),
        Err(_) => return HttpReply::error(500, "backend lock poisoned"),
    };
    HttpReply::ok(&serde_json::json!({ "provisioned": provisioned }))
}

#[cfg(test)]
mod tests {
    // Bare variant imports (the `Admin` variant, not its fully-qualified path)
    // keep the capability-grep gate (V1.1a) clean — that qualified spelling is
    // reserved for capabilities/types/tests.rs.
    use super::*;
    use crate::services::auth::store::AuthStore;
    use crate::services::auth::types::UserRole;
    use crate::services::auth::types::UserRole::{Admin, Manager, Superadmin, User as Regular};
    use std::path::PathBuf;
    use std::time::Duration;

    /// Build a bare [`User`] with the given role — only `role` gates these
    /// handlers, so the other fields are placeholders.
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

    /// A `Mutex<Backend>` with auth enabled over a leaked temp dir (so the
    /// SQLite file + identity/flag paths outlive the test body), mirroring the
    /// fixture in `transport/maint/mod.rs`.
    fn backend() -> Mutex<Backend> {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = AuthStore::open(&dir.path().join("auth.db")).expect("open auth store");
        let backend = Backend::new(
            dir.path().to_path_buf(),
            100.0,
            PathBuf::from("/tmp/cp-it-test-realms"),
            PathBuf::from("/tmp/cp-it-test-bin"),
            Some(store),
            Duration::from_secs(3600),
        );
        std::mem::forget(dir);
        Mutex::new(backend)
    }

    /// V4.1a — every JSON `/api/it/*` handler is gated on `can_manage_it`:
    /// `manager`/`user` → 403; `admin`/`superadmin` → the handler's own
    /// success/non-403 status (a real caller below the bar is refused, one at or
    /// above passes into the delegate).
    #[test]
    fn it_gated() {
        let state = backend();
        for role in [Manager, Regular] {
            let u = user(role);
            assert_eq!(it_ca_fingerprint(Some(&u)).status, 403, "ca/fingerprint denied for {role:?}");
            assert_eq!(it_get_identity(&state, Some(&u)).status, 403, "identity GET denied for {role:?}");
            assert_eq!(
                it_set_identity(&state, br#"{"name":"box","ip":"192.168.1.1"}"#, Some(&u)).status,
                403,
                "identity POST denied for {role:?}"
            );
            assert_eq!(it_provisioned(&state, Some(&u)).status, 403, "provisioned denied for {role:?}");
        }
        for role in [Admin, Superadmin] {
            let u = user(role);
            // `ca/fingerprint` 404s in the test env (no CP_CA_ROOT) — the point
            // is the gate lets the caller reach the delegate, i.e. not a 403.
            assert_ne!(it_ca_fingerprint(Some(&u)).status, 403, "ca/fingerprint reached for {role:?}");
            assert_eq!(it_get_identity(&state, Some(&u)).status, 200, "identity GET ok for {role:?}");
            assert_eq!(it_provisioned(&state, Some(&u)).status, 200, "provisioned ok for {role:?}");
        }
    }

    /// V4.1b — a valid name/IP round-trips through set→get; an invalid IP and an
    /// invalid name are each rejected with `400`. Driven with `auth_user == None`
    /// (god-mode) so this exercises the identity logic, not the gate.
    #[test]
    fn it_identity_roundtrip() {
        let state = backend();
        // No identity initially.
        assert!(it_get_identity(&state, None).body.contains("\"identity\":null"), "no identity initially");

        // Valid → 200, then GET reflects it.
        let set = it_set_identity(&state, br#"{"name":"pilot.acme.corp","ip":"192.168.1.116"}"#, None);
        assert_eq!(set.status, 200, "valid identity accepted");
        let got = it_get_identity(&state, None);
        assert_eq!(got.status, 200);
        assert!(
            got.body.contains("pilot.acme.corp") && got.body.contains("192.168.1.116"),
            "GET reflects the set identity: {}",
            got.body
        );

        // Invalid IP and invalid name are each a 400.
        assert_eq!(it_set_identity(&state, br#"{"name":"box","ip":"nope"}"#, None).status, 400, "bad IP → 400");
        assert_eq!(
            it_set_identity(&state, br#"{"name":"-bad.example","ip":"10.0.0.1"}"#, None).status,
            400,
            "bad name → 400"
        );
    }
}
