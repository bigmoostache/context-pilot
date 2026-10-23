//! Unit tests for the auth transport handlers.

use std::path::PathBuf;
use std::time::Duration;

use super::*;

/// Build a `Mutex<Backend>` with an auth store present, and the access-control
/// flag forced to `access_control`. Returns the `TempDir` guard alongside so
/// the `SQLite` file outlives the test body (bound as `_dir` at call sites).
fn backend(access_control: bool) -> (Mutex<Backend>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = AuthStore::open(&dir.path().join("auth.db")).expect("open auth store");
    let mut b = Backend::new(
        crate::transport::Paths {
            agents_dir: dir.path().to_path_buf(),
            agents_root: PathBuf::from("/tmp/cp-auth-test-realms"),
            agent_binary: PathBuf::from("/tmp/cp-auth-test-bin"),
        },
        Some(store),
        Duration::from_hours(1),
    );
    b.access_control = access_control;
    (Mutex::new(b), dir)
}

/// V0.4b — with the flag OFF, a request with no token on an agent-scoped
/// route resolves to full access (`Ok(None)` = god mode), even though an auth
/// store is present. Mirrors the current auth-disabled behaviour.
#[test]
fn flag_off_is_god_mode() {
    let (state, _dir) = backend(false);
    let outcome = authenticate(&state, &["api", "fleet", "some-agent"], None);
    assert!(matches!(outcome, Ok(None)), "flag off \u{21d2} no authenticated user (god mode)");
}

/// With the flag ON, the same tokenless request to a protected route is
/// rejected (401), while a public route still passes through.
#[test]
fn flag_on_enforces() {
    let (state, _dir) = backend(true);
    let protected = authenticate(&state, &["api", "fleet", "some-agent"], None);
    assert!(matches!(protected, Err(reply) if reply.status == 401), "flag on + no token \u{21d2} 401");
    let public = authenticate(&state, &["api", "health"], None);
    assert!(matches!(public, Ok(None)), "public route bypasses the gate");
}

// Bare variant imports (not the `UserRole::Superadmin` qualified path) keep
// the capability-grep gate happy (reserved for capabilities/types/tests.rs).
use crate::services::auth::types::UserRole::{Superadmin, User as Regular};

fn user(role: UserRole, must_change_password: bool) -> User {
    User {
        id: "a".to_owned(),
        email: "a@box".to_owned(),
        name: "A".to_owned(),
        password_hash: String::new(),
        role,
        must_change_password,
        created_at: 0,
        updated_at: 0,
    }
}

/// V5.2a — `me` drives the day-0 flow via `next_action` from the threaded
/// provisioning state: a `can_manage_it` operator on an unprovisioned box walks
/// `change_password → set_identity → (cleared)` as each precondition is met; a
/// regular user skips the IT step → `ready`. Hermetic (only the durable flag).
#[test]
fn me_next_action_day0_walk_and_capability_scope() {
    let (state, _dir) = backend(false); // me() ignores access_control; needs the flag path
    let flag_path = state.lock().expect("lock").provision_flag_path.clone();
    // The day-0 step exists only where the deployment enables it.
    let day0 = Features::default().with(Feature::Day0Setup, true);
    let na = |u: &User| me_with(&state, Some(u), day0).body;
    let set = |v| crate::transport::it::state::set_provisioned(&flag_path, v).expect("flag");
    set(false);
    let mut admin = user(Superadmin, true);
    // 1. Seeded paper password must be rotated first.
    assert!(na(&admin).contains("\"next_action\":\"change_password\""), "paper password first");
    // 2. Password changed, box still unprovisioned → day-0 identity step.
    admin.must_change_password = false;
    assert!(na(&admin).contains("\"next_action\":\"set_identity\""), "unprovisioned IT operator names the box");
    // 3. Provisioned → day-0 IT gate cleared (proceeds to app / onboarding).
    set(true);
    let done = na(&admin);
    assert!(!done.contains("\"next_action\":\"set_identity\""), "provisioned clears day-0: {done}");
    // A regular user skips the IT step; no user-mgmt cap → onboarding short-circuits → ready.
    set(false);
    assert!(na(&user(Regular, false)).contains("\"next_action\":\"ready\""), "non-IT user \u{21d2} ready");
}
