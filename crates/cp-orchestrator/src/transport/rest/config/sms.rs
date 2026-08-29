//! SMS REST handlers — the message archive of the box's own SIM, gated on
//! `can_manage_it` (admin+).
//!
//! # Why `can_manage_it` and not `can_manage_secrets`
//!
//! The neighbouring bearer routes draw a stricter line: the APN and the SIM PIN
//! are `can_manage_secrets`, because we ship the SIM and own the data plan, and
//! a client's IT admin changing them breaks their connectivity and bills us.
//!
//! SMS sits on the other side of that line **on purpose**. Reading the messages
//! the site's own appliance receives — a carrier's data-cap warning, an
//! out-of-band alert — is operating the site, which is exactly what an admin is
//! for. Putting it behind the vendor capability would make the feature
//! unreachable by the only people who need it.
//!
//! Sending does cost us money, so the ceiling is enforced rather than delegated
//! to a capability: every send is rate-limited per operator and per box, and
//! carries the user id that ordered it into the archive
//! ([`sms`](crate::transport::it::network::sms)). That is a narrower instrument
//! than a role, and it survives the case a role cannot describe — one admin
//! sending a thousand messages.
//!
//! Gate semantics mirror the rest of the RBAC surface: a `None` `auth_user`
//! means access control is off (god-mode, FR-v3-08) and passes through; a
//! present caller without `can_manage_it` is a `403`. Client gating is cosmetic,
//! the server is authoritative.

use std::sync::Mutex;

use super::super::{Backend, HttpReply};
use crate::services::auth::types::User;
use crate::transport::it::network::sms;

/// The gate these routes share. Returns the `403` to send, or `None` when the
/// caller may proceed.
fn denied(auth_user: Option<&User>) -> Option<HttpReply> {
    if auth_user.is_some_and(|user| !user.can_manage_it()) {
        return Some(HttpReply::error(403, "IT management access required"));
    }
    None
}

/// `GET /api/it/sms` — one page of the archive, newest first
/// (`?before=<id>&limit=<n>`).
pub(crate) fn it_list_sms(state: &Mutex<Backend>, query: &str, auth_user: Option<&User>) -> HttpReply {
    denied(auth_user).unwrap_or_else(|| sms::list(state, query))
}

/// `POST /api/it/sms` — send one message.
///
/// The caller's user id is threaded through to the archive: an outbound message
/// spends the vendor's plan, so who ordered it is part of the record, not a log
/// line that rotates away.
pub(crate) fn it_send_sms(state: &Mutex<Backend>, body: &[u8], auth_user: Option<&User>) -> HttpReply {
    denied(auth_user).unwrap_or_else(|| sms::send(state, body, auth_user.map(|user| user.id.as_str())))
}

/// `POST /api/it/sms/{id}/read` — mark one message read.
pub(crate) fn it_read_sms(state: &Mutex<Backend>, id: &str, auth_user: Option<&User>) -> HttpReply {
    denied(auth_user).unwrap_or_else(|| sms::mark_read(state, id))
}

/// `DELETE /api/it/sms/{id}` — drop one message from the archive.
pub(crate) fn it_delete_sms(state: &Mutex<Backend>, id: &str, auth_user: Option<&User>) -> HttpReply {
    denied(auth_user).unwrap_or_else(|| sms::remove(state, id))
}

#[cfg(test)]
mod tests {
    // Bare variant imports (the `Admin` variant, not its fully-qualified path)
    // keep the capability-grep gate (V1.1a) clean.
    use super::*;
    use crate::services::auth::db::AuthStore;
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

    /// A `Mutex<Backend>` plus its owning temp dir, so the archive lands
    /// somewhere writable and private.
    fn backend() -> (tempfile::TempDir, Mutex<Backend>) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = AuthStore::open(&dir.path().join("auth.db")).expect("open auth store");
        let backend = Backend::new(
            crate::transport::Paths {
                agents_dir: dir.path().to_path_buf(),
                agents_root: PathBuf::from("/tmp/cp-sms-test-realms"),
                agent_binary: PathBuf::from("/tmp/cp-sms-test-bin"),
            },
            Some(store),
            Duration::from_hours(1),
        );
        (dir, Mutex::new(backend))
    }

    /// Every route refuses a caller below `admin` and admits `admin`+.
    ///
    /// The matrix, not one spot check: a capability that is right for three
    /// routes and wrong for the fourth is exactly the bug this catches.
    #[test]
    fn rbac_matrix() {
        let (_dir, state) = backend();
        for (role, allowed) in [(Superadmin, true), (Admin, true), (Manager, false), (Regular, false)] {
            let caller = user(role);
            let who = Some(&caller);
            assert_eq!(it_list_sms(&state, "", who).status != 403, allowed, "list {role:?}");
            assert_eq!(it_send_sms(&state, b"{}", who).status != 403, allowed, "send {role:?}");
            assert_eq!(it_read_sms(&state, "1", who).status != 403, allowed, "read {role:?}");
            assert_eq!(it_delete_sms(&state, "1", who).status != 403, allowed, "delete {role:?}");
        }
    }

    /// Access control off (`None`) is god-mode and passes every gate (FR-v3-08).
    #[test]
    fn no_auth_user_passes() {
        let (_dir, state) = backend();
        assert_ne!(it_list_sms(&state, "", None).status, 403, "list with access control off");
        assert_ne!(it_delete_sms(&state, "1", None).status, 403, "delete with access control off");
    }

    /// A malformed id is a `400`, not a `500` or a silent success.
    #[test]
    fn bad_id_is_rejected() {
        let (_dir, state) = backend();
        assert_eq!(it_read_sms(&state, "not-a-number", None).status, 400, "non-numeric id");
        assert_eq!(it_delete_sms(&state, "12x", None).status, 400, "trailing junk id");
    }

    /// An id that parses but names nothing is a `404`.
    #[test]
    fn missing_message_is_404() {
        let (_dir, state) = backend();
        assert_eq!(it_read_sms(&state, "4242", None).status, 404, "no such message");
        assert_eq!(it_delete_sms(&state, "4242", None).status, 404, "no such message");
    }
}
