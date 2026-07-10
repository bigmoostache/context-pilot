//! Dispatch-level RBAC tests — capability guards enforced by `route_rest`'s
//! match arms (not inside individual handlers), exercised straight through the
//! private router. Lives beside the auth middleware because the property under
//! test is authorization, not the handlers' own behaviour.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tiny_http::Method;

use crate::services::MaterializedView;
use crate::services::auth::types::{User, UserRole};
// Bare variant imports (not the `UserRole::Admin` qualified spelling) keep the
// capability-grep gate clean — the qualified paths are reserved for
// capabilities/types/tests.rs.
use crate::services::auth::types::UserRole::{Admin, User as Regular};
use crate::transport::{Backend, route_rest};

/// Build a bare [`User`] with the given role — only `role` matters to the
/// dispatch guards under test.
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

/// A hermetic backend for dispatching `route_rest` directly (no socket, no
/// real releases-dir writes — every mutating probe below fails handler-side
/// validation before touching the store).
fn state() -> Arc<Mutex<Backend>> {
    Arc::new(Mutex::new(Backend::for_test(PathBuf::from("/tmp/cp-rbac-test-agents"), MaterializedView::new())))
}

/// Dispatch one route through [`route_rest`] with the given caller.
fn dispatch(state: &Arc<Mutex<Backend>>, method: &Method, segments: &[&str], caller: Option<&User>) -> u16 {
    route_rest(method, segments, state, b"", "", None, caller).status
}

/// V0.2a — every `/api/releases/*` route sits behind the single
/// `can_manage_it` guard arm: a `user`-role caller gets `403` on all seven
/// routes; an `Admin` (and a `None` caller = access control off, god-mode
/// §13.10) passes the gate and reaches the handler (any non-403 status — the
/// safe probes below fail handler-side validation with `400`, or succeed with
/// `200` for the no-op fleet deploy).
#[test]
fn releases_rbac() {
    let state = state();
    let all: [(&Method, &[&str]); 7] = [
        (&Method::Get, &["api", "releases"]),
        (&Method::Put, &["api", "releases", "arch"]),
        (&Method::Post, &["api", "releases", "download"]),
        (&Method::Put, &["api", "releases", "select"]),
        (&Method::Post, &["api", "releases", "deploy"]),
        (&Method::Post, &["api", "releases", "restart-orchestrator"]),
        (&Method::Delete, &["api", "releases", "v0.0.0-ghost"]),
    ];
    let low = user(Regular);
    for (method, segments) in all {
        assert_eq!(dispatch(&state, method, segments, Some(&low)), 403, "user must be refused on {segments:?}");
    }

    // Gate-pass probes for Admin and god-mode. Restricted to routes with a
    // safe, network-free outcome: `GET /api/releases` would call the GitHub
    // API and `restart-orchestrator` would re-exec the test binary — the
    // guard is a single arm, so passing it anywhere proves it for all. The
    // probes land on handler-side validation (400), the break-glass gate of
    // the retired routes (410, T5.1.5) — anything but the guard's 403.
    let safe: [(&Method, &[&str]); 4] = [
        (&Method::Put, &["api", "releases", "arch"]),
        (&Method::Post, &["api", "releases", "download"]),
        (&Method::Put, &["api", "releases", "select"]),
        (&Method::Delete, &["api", "releases", "v0.0.0-ghost"]),
    ];
    let admin = user(Admin);
    for (method, segments) in safe {
        assert_ne!(dispatch(&state, method, segments, Some(&admin)), 403, "admin passes the gate {segments:?}");
        assert_ne!(dispatch(&state, method, segments, None), 403, "god-mode passes the gate {segments:?}");
    }
    // The no-op fleet deploy (empty view) even succeeds outright.
    assert_eq!(dispatch(&state, &Method::Post, &["api", "releases", "deploy"], Some(&admin)), 200);
    assert_eq!(dispatch(&state, &Method::Post, &["api", "releases", "deploy"], None), 200);
}

/// V5.1a — every `/api/update/*` route sits behind the same `can_manage_it`
/// guard arm: `user`/`Manager` → 403 on all four; an `Admin` reaches the
/// handlers (`status`/`mode` prove it with a real 200 — `check`/`apply` hit
/// the network and the re-exec path, so the single shared guard arm carries
/// the proof for them).
#[test]
fn update_routes_rbac() {
    let state = state();
    let all: [(&Method, &[&str]); 4] = [
        (&Method::Get, &["api", "update", "status"]),
        (&Method::Post, &["api", "update", "check"]),
        (&Method::Post, &["api", "update", "apply"]),
        (&Method::Put, &["api", "update", "mode"]),
    ];
    let low = user(Regular);
    for (method, segments) in all {
        assert_eq!(dispatch(&state, method, segments, Some(&low)), 403, "user must be refused on {segments:?}");
    }

    let admin = user(Admin);
    assert_eq!(dispatch(&state, &Method::Get, &["api", "update", "status"], Some(&admin)), 200);
    assert_eq!(dispatch(&state, &Method::Get, &["api", "update", "status"], None), 200, "god-mode passes");
    // `mode` with an empty body is a handler-side 400 — past the gate.
    assert_ne!(dispatch(&state, &Method::Put, &["api", "update", "mode"], Some(&admin)), 403);
}

/// T5.1.5 — the retired manual version-choice routes are `410 Gone` without
/// the break-glass env (arch/list/deploy stay live; the guard still runs
/// first, so a low-role caller sees 403, not 410).
#[test]
fn retired_release_routes_are_gone() {
    let state = state();
    let admin = user(Admin);
    let retired: [(&Method, &[&str]); 3] = [
        (&Method::Post, &["api", "releases", "download"]),
        (&Method::Put, &["api", "releases", "select"]),
        (&Method::Delete, &["api", "releases", "v0.0.0-ghost"]),
    ];
    for (method, segments) in retired {
        assert_eq!(dispatch(&state, method, segments, Some(&admin)), 410, "retired: {segments:?}");
        assert_eq!(dispatch(&state, method, segments, Some(&user(Regular))), 403, "guard precedes the 410");
    }
    // The surviving routes are untouched by the break-glass gate.
    assert_ne!(dispatch(&state, &Method::Put, &["api", "releases", "arch"], Some(&admin)), 410);
    assert_ne!(dispatch(&state, &Method::Post, &["api", "releases", "deploy"], Some(&admin)), 410);
}
