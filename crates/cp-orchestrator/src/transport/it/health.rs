//! `/healthz` — the box's readiness probe (update-policy §5.2/§5.5).
//!
//! `200` only when the process is genuinely serving: the socket is bound (we
//! are answering at all), the auth database answers a query, and the agent
//! registry directory is readable. Anything less is `503` — which is exactly
//! what keeps a staged self-update from being committed (the health-gated
//! `boot_commit` polls this endpoint and only clears the rollback markers on a
//! `200`).
//!
//! The route is top-level (`/healthz`, not `/api/...`), unauthenticated, and
//! **loopback-only** (enforced by the dispatcher in
//! [`transport::handle`](crate::transport)): its consumer is the box itself.
//! The body carries **booleans only** — no token, path, or any other detail an
//! unauthenticated caller shouldn't see.

use std::sync::Mutex;

use crate::transport::rest::{Backend, HttpReply};

/// `GET /healthz` — `200` when every readiness check passes, else `503`.
///
/// Checks (socket-bound is inherent — this handler ran):
/// * `auth_db` — when auth is enabled, the SQLite store answers a query
///   (an unconfigured store passes: no database is required to be open);
/// * `registry` — the agents directory is readable.
pub(crate) fn healthz(state: &Mutex<Backend>) -> HttpReply {
    let Ok(b) = state.lock() else {
        return HttpReply { status: 503, body: "{\"status\":\"unavailable\"}".to_owned() };
    };
    let auth_db = b.auth.as_ref().map_or(true, |auth| auth.count_users().is_ok());
    let registry = std::fs::read_dir(&b.agents_dir).is_ok();
    drop(b);

    let healthy = auth_db && registry;
    let body = serde_json::json!({
        "status": if healthy { "ok" } else { "unavailable" },
        "checks": { "auth_db": auth_db, "registry": registry },
    });
    HttpReply { status: if healthy { 200 } else { 503 }, body: body.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::MaterializedView;
    use crate::services::auth::store::AuthStore;
    use std::path::PathBuf;
    use std::time::Duration;

    /// A backend over a real tempdir (readable registry) with auth enabled on
    /// a real SQLite file. The tempdir is leaked so paths outlive the test.
    fn backend_with_auth() -> (Mutex<Backend>, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("auth.db");
        let store = AuthStore::open(&db_path).expect("open auth store");
        let backend = Backend::new(
            dir.path().to_path_buf(),
            PathBuf::from("/tmp/cp-health-test-realms"),
            PathBuf::from("/tmp/cp-health-test-bin"),
            Some(store),
            Duration::from_secs(3600),
        );
        std::mem::forget(dir);
        (Mutex::new(backend), db_path)
    }

    /// V2.1a (handler half) — a bound, DB-open, registry-readable backend is
    /// `200 ok`; with auth disabled entirely it is healthy too.
    #[test]
    fn healthy_backend_is_200() {
        let (state, _db) = backend_with_auth();
        let reply = healthz(&state);
        assert_eq!(reply.status, 200, "healthy backend: {}", reply.body);
        assert!(reply.body.contains("\"status\":\"ok\""));

        let dir = tempfile::tempdir().expect("tempdir");
        let no_auth = Mutex::new(Backend::for_test(dir.path().to_path_buf(), MaterializedView::new()));
        assert_eq!(healthz(&no_auth).status, 200, "auth disabled is still healthy");
    }

    /// V2.1b — the auth database deliberately made unable to answer (its
    /// tables dropped underneath the open store) → `503`, `auth_db: false`.
    #[test]
    fn broken_auth_db_is_503() {
        let (state, db_path) = backend_with_auth();
        // Sever the schema through a second connection to the same file — the
        // store's own handle stays open, but its queries now fail.
        let conn = rusqlite::Connection::open(&db_path).expect("second connection");
        conn.execute_batch("DROP TABLE users;").expect("drop users table");

        let reply = healthz(&state);
        assert_eq!(reply.status, 503, "severed DB must be unavailable: {}", reply.body);
        assert!(reply.body.contains("\"auth_db\":false"));
    }

    /// An unreadable agents directory (registry not loadable) → `503`.
    #[test]
    fn missing_registry_dir_is_503() {
        let state =
            Mutex::new(Backend::for_test(PathBuf::from("/tmp/cp-health-no-such-dir-x9"), MaterializedView::new()));
        let reply = healthz(&state);
        assert_eq!(reply.status, 503, "unreadable registry: {}", reply.body);
        assert!(reply.body.contains("\"registry\":false"));
    }

    /// V2.1c — the body is booleans + a status word only: no token, no
    /// absolute path, no secret ever leaves this unauthenticated endpoint.
    #[test]
    fn body_carries_no_sensitive_data() {
        let (state, db_path) = backend_with_auth();
        for reply in [healthz(&state), {
            let conn = rusqlite::Connection::open(&db_path).expect("second connection");
            conn.execute_batch("DROP TABLE users;").expect("drop users table");
            healthz(&state)
        }] {
            let parsed: serde_json::Value = serde_json::from_str(&reply.body).expect("healthz body is JSON");
            let obj = parsed.as_object().expect("object body");
            assert_eq!(obj.keys().collect::<Vec<_>>(), ["checks", "status"], "only status + checks keys");
            assert!(parsed["status"].is_string());
            let checks = parsed["checks"].as_object().expect("checks object");
            assert!(checks.values().all(serde_json::Value::is_boolean), "checks are booleans only");
            assert!(!reply.body.contains('/'), "no path fragment in the body: {}", reply.body);
        }
    }
}
