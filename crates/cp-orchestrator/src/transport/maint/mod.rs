//! IT **maintenance plane** — the second HTTP face, served on its own listener
//! (`:9090` by default) and isolated from the product cockpit (`:7878`).
//!
//! This plane is the appliance's provisioning console (architecture memory
//! `local-tls-onboarding`): it carries the routes the IT operator needs to bring
//! a fresh box online — forced password/email change, name/IP identity, the
//! private-CA download, and the `finalize` that flips the box to *provisioned* —
//! and **nothing else**. No fleet, agent, or chat route is reachable here, so a
//! foothold on `:9090` cannot drive the product API.
//!
//! Two guards stack on top of the network isolation:
//!
//! * **LAN-only** ([`lan_allowed`]) — the maintenance plane is meant to be
//!   reached over the local network only. A request whose peer address is a
//!   public IP is refused before any handler runs. Controlled by
//!   `CP_MAINT_LAN_ONLY` (default on); the box additionally binds/firewalls the
//!   listener to its LAN interface (documented in the procd `.init`).
//! * **Admin-gated** ([`authenticate`]) — every route except the unauthenticated
//!   whitelist (`login`, `status`) requires a valid session whose user holds the
//!   [`UserRole::Admin`] role. A `User`-role token is a `403`, no token a `401`.

mod ca;
mod caddy;
mod crypto;
mod identity;
mod state;

pub(crate) use identity::apply_caddy_at_boot;
pub(crate) use state::is_provisioned;

use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use tiny_http::{Header, Method, Request, Response};

use super::Backend;
use super::rest::HttpReply;
use crate::services::auth::types::{User, UserRole};

/// Handle one request on the maintenance plane: LAN guard → Admin gate → route.
/// Mirrors the product [`super::handle`] but dispatches only maintenance routes,
/// so the product API surface is structurally absent here.
///
/// Unlike the product plane, the maintenance plane emits **no CORS headers**
/// (see [`respond`]): the maintenance UI is served same-origin from this very
/// listener, so a cross-origin caller — a malicious LAN page, a DNS-rebinding
/// attempt — is stopped by the browser's same-origin policy instead of being
/// waved through by a wildcard `Access-Control-Allow-Origin`.
pub(crate) fn handle(mut request: Request, state: &Arc<Mutex<Backend>>) {
    // Network isolation guard (M1.1.2): the maintenance plane only answers the
    // LAN. A public-IP peer never reaches a handler.
    if !lan_allowed(&request) {
        respond(request, &HttpReply::error(403, "maintenance plane is LAN-only"));
        return;
    }

    let (path, _query) = super::split_url(request.url());
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let method = request.method().clone();

    let auth_token = request.headers().iter().find_map(|h| {
        if h.field.equiv("Authorization") { h.value.as_str().strip_prefix("Bearer ").map(str::to_owned) } else { None }
    });

    // Admin gate — every non-whitelisted route needs an Admin session.
    let auth_user = match authenticate(state, &segments, auth_token.as_deref()) {
        Ok(user) => user,
        Err(reply) => {
            respond(request, &reply);
            return;
        }
    };

    // Raw-bytes route: the CA root download needs a non-JSON content type, so it
    // owns the request directly. Admin is already enforced (ca.crt is not in the
    // public whitelist), and GET carries no body.
    if method == Method::Get && segments.as_slice() == ["api", "maint", "ca.crt"] {
        ca::serve_ca_cert(request);
        return;
    }

    let body_bytes = if matches!(method, Method::Post | Method::Patch | Method::Put) {
        super::read_body(&mut request)
    } else {
        Vec::new()
    };

    let reply = route(&method, &segments, state, body_bytes.as_slice(), auth_token.as_deref(), auth_user.as_ref());
    respond(request, &reply);
}

/// Send a JSON reply on the maintenance plane, deliberately **without** CORS
/// headers. The plane is same-origin (UI + API both on `:9090`) and privileged,
/// so it must not advertise a permissive cross-origin policy.
fn respond(request: Request, reply: &HttpReply) {
    let mut response = Response::from_string(&reply.body).with_status_code(reply.status);
    if let Ok(header) = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]) {
        response = response.with_header(header);
    }
    let _sent = request.respond(response);
}

/// Dispatch a maintenance route. Returns `404` for anything not in the
/// maintenance surface — this is what keeps the product API (fleet, agents,
/// chat) structurally unreachable on `:9090`.
fn route(
    method: &Method,
    segments: &[&str],
    state: &Mutex<Backend>,
    body: &[u8],
    auth_token: Option<&str>,
    auth_user: Option<&User>,
) -> HttpReply {
    match (method, segments) {
        // Unauthenticated whitelist.
        (Method::Get, ["api", "maint", "status"]) => status(state),
        (Method::Post, ["api", "maint", "login"]) => super::auth::login(state, body),

        // Admin-gated session/profile routes (reuse the product auth handlers).
        (Method::Get, ["api", "maint", "me"]) => super::auth::me(auth_user),
        (Method::Post, ["api", "maint", "password"]) => super::auth::change_password(state, body, auth_user),
        (Method::Patch, ["api", "maint", "me"]) => super::auth::update_me(state, body, auth_user),
        (Method::Post, ["api", "maint", "logout"]) => super::auth::logout(state, auth_token),

        // Box identity + private-CA TLS (M3).
        (Method::Get, ["api", "maint", "identity"]) => identity::get_identity(state),
        (Method::Post, ["api", "maint", "identity"]) => identity::set_identity(state, body),

        // Private-CA root distribution (M4). `ca.crt` itself is served as raw
        // bytes in `handle` (it needs a non-JSON content type); here is its
        // fingerprint for out-of-band verification.
        (Method::Get, ["api", "maint", "ca", "fingerprint"]) => ca::ca_fingerprint(),

        // Provisioning state machine (M2).
        (Method::Post, ["api", "maint", "finalize"]) => finalize(state, auth_user),

        _ => HttpReply { status: 404, body: "{\"error\":\"not found\"}".to_owned() },
    }
}

/// Authenticate a maintenance-plane request, additionally requiring the
/// [`UserRole::Admin`] role on every protected route.
///
/// * Whitelisted route (`login`, `status`) → `Ok(None)`.
/// * No token → `Err(401)`.
/// * Valid session, `User` role → `Err(403)`.
/// * Valid session, `Admin` role → `Ok(Some(user))`.
/// * Auth disabled → `Err(503)` on protected routes (the maintenance plane is
///   meaningless without the role model it gates on).
fn authenticate(
    state: &Mutex<Backend>,
    segments: &[&str],
    auth_token: Option<&str>,
) -> Result<Option<User>, HttpReply> {
    if is_public_route(segments) {
        return Ok(None);
    }

    let auth_enabled = state.lock().map(|b| b.auth.is_some()).unwrap_or(false);
    if !auth_enabled {
        return Err(HttpReply::error(503, "maintenance plane requires auth to be enabled"));
    }

    let Some(token) = auth_token else {
        return Err(HttpReply::error(401, "missing authorization"));
    };

    let b = state.lock().map_err(|_| HttpReply::error(500, "backend lock poisoned"))?;
    let auth = b.auth.as_ref().ok_or_else(|| HttpReply::error(503, "auth not enabled"))?;
    let user = match auth.validate_session(token) {
        Ok(Some(user)) => user,
        Ok(None) => return Err(HttpReply::error(401, "invalid or expired session")),
        Err(_) => return Err(HttpReply::error(500, "session validation error")),
    };

    if user.role == UserRole::Admin { Ok(Some(user)) } else { Err(HttpReply::error(403, "admin access required")) }
}

/// Routes reachable on the maintenance plane without authentication. Kept to the
/// minimum the IT operator needs before holding a session: the login itself and
/// a status probe (so the wizard can render before any token exists).
fn is_public_route(segments: &[&str]) -> bool {
    matches!(segments, ["api", "maint", "status"] | ["api", "maint", "login"])
}

/// `GET /api/maint/status` — public probe describing the maintenance plane's
/// readiness. Lets the wizard decide what to render before any login: whether an
/// admin exists yet (`bootstrapped`) and whether the box is already provisioned
/// (so a re-visited, live box can show the post-provisioning view).
fn status(state: &Mutex<Backend>) -> HttpReply {
    let (bootstrapped, provisioned, identity_set) = state
        .lock()
        .map(|b| {
            let bootstrapped = b.auth.as_ref().and_then(|a| a.count_users().ok()).map_or(false, |n| n > 0);
            let identity_set = identity::load_identity(&identity::identity_path(&b.agents_dir)).is_some();
            (bootstrapped, is_provisioned(&b.provision_flag_path), identity_set)
        })
        .unwrap_or((false, false, false));
    HttpReply::ok(&serde_json::json!({
        "plane": "maintenance",
        "bootstrapped": bootstrapped,
        "provisioned": provisioned,
        // The name/IP itself is not exposed pre-login; only whether it is set, so
        // the wizard can skip or prefill the identity step (Admin GETs the value).
        "identity_set": identity_set,
    }))
}

/// `POST /api/maint/finalize` (Admin) — flip the box to *provisioned* and start
/// serving the cockpit.
///
/// Pre-requisites (Obj 5.4): the seeded paper password must have been changed
/// (`must_change_password == false`) **and** the box identity (name/IP) must be
/// set — the leaf needs subjects before the cockpit can be trusted. On success
/// it persists the durable flag, then re-renders the Caddyfile in *provisioned*
/// mode and reloads Caddy so `:443` begins serving the cockpit (Obj 2.2.2).
/// Idempotent: finalizing an already-provisioned box re-writes the same flag and
/// re-applies the same Caddy config.
fn finalize(state: &Mutex<Backend>, auth_user: Option<&User>) -> HttpReply {
    // The Admin gate guarantees `auth_user` is `Some(Admin)` here.
    let Some(caller) = auth_user else {
        return HttpReply::error(401, "admin authorization required");
    };

    // Pre-requisite: the seeded paper password must have been changed.
    if caller.must_change_password {
        return HttpReply::error(412, "change the admin password before finalizing");
    }

    let (flag_path, identity) = match state.lock() {
        Ok(b) => (b.provision_flag_path.clone(), identity::load_identity(&identity::identity_path(&b.agents_dir))),
        Err(_) => return HttpReply::error(500, "backend lock poisoned"),
    };

    // Pre-requisite: the box must be named (a leaf needs subjects).
    let Some(identity) = identity else {
        return HttpReply::error(412, "set the box name/IP before finalizing");
    };

    if let Err(e) = state::set_provisioned(&flag_path, true) {
        eprintln!("finalize: could not persist provisioned flag: {e}");
        return HttpReply::error(500, "could not persist provisioned flag");
    }

    // Re-render Caddy in provisioned mode → :443 starts serving the cockpit.
    match caddy::regenerate(true, Some(&identity)) {
        Ok(reloaded) => HttpReply::ok(&serde_json::json!({ "provisioned": true, "reloaded": reloaded })),
        Err(e) => {
            eprintln!("finalize: caddy reload failed: {e}");
            // The box is provisioned (flag persisted); a retry/identity change
            // will re-apply the cockpit config.
            HttpReply::error(502, "provisioned but the TLS reload failed")
        }
    }
}

/// Whether the request's peer is permitted by the LAN-only guard.
///
/// When `CP_MAINT_LAN_ONLY` is off, every peer is allowed (useful behind an
/// external firewall that already enforces reachability). Otherwise only
/// loopback and RFC-1918 / link-local / unique-local addresses pass; a public
/// address is refused. A missing peer address (should not happen on `tiny_http`)
/// is treated as not-LAN and refused when the guard is on.
fn lan_allowed(request: &Request) -> bool {
    if !lan_only_enabled() {
        return true;
    }
    match request.remote_addr() {
        Some(addr) => is_lan_addr(addr.ip()),
        None => false,
    }
}

/// Read `CP_MAINT_LAN_ONLY` (default on). `0`/`false` disables the guard.
fn lan_only_enabled() -> bool {
    !std::env::var("CP_MAINT_LAN_ONLY").map(|s| s == "0" || s.eq_ignore_ascii_case("false")).unwrap_or(false)
}

/// Whether `ip` belongs to the local network (loopback, private, link-local, or
/// IPv6 unique-local) rather than the public internet.
fn is_lan_addr(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            // `is_unique_local`/`is_unicast_link_local` are unstable on stable
            // Rust, so test the fc00::/7 and fe80::/10 prefixes by hand.
            let seg0 = v6.segments()[0];
            v6.is_loopback()
                || (seg0 & 0xfe00) == 0xfc00
                || (seg0 & 0xffc0) == 0xfe80
                || v6.to_ipv4_mapped().is_some_and(is_lan_v4)
        }
    }
}

/// IPv4 LAN predicate, shared with the IPv4-mapped IPv6 path.
fn is_lan_v4(v4: std::net::Ipv4Addr) -> bool {
    v4.is_loopback() || v4.is_private() || v4.is_link_local()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};
    use std::path::PathBuf;
    use std::time::Duration;

    use crate::services::auth::store::AuthStore;

    /// Build a `Mutex<Backend>` with auth enabled and two seeded users, returning
    /// their session tokens so the gate can be exercised per role (Objective
    /// 1.3.2 — 401/403/200 by role).
    fn backend_with_users() -> (Mutex<Backend>, String, String) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = AuthStore::open(&dir.path().join("auth.db")).expect("open auth store");
        let admin = store.create_user("admin@box", "Admin", "password1", UserRole::Admin).expect("admin");
        let user = store.create_user("user@box", "User", "password1", UserRole::User).expect("user");
        let ttl = Duration::from_secs(3600);
        let admin_tok = store.create_session(&admin.id, None, ttl).expect("admin session");
        let user_tok = store.create_session(&user.id, None, ttl).expect("user session");
        let backend = Backend::new(
            dir.path().to_path_buf(),
            100.0,
            PathBuf::from("/tmp/cp-maint-test-realms"),
            PathBuf::from("/tmp/cp-maint-test-bin"),
            Some(store),
            ttl,
        );
        // Leak the tempdir so the SQLite file outlives the test body.
        std::mem::forget(dir);
        (Mutex::new(backend), admin_tok, user_tok)
    }

    /// Build a `Mutex<Backend>` with auth enabled, returning the seeded admin
    /// `User` (whose `must_change_password` is false by default) so the finalize
    /// pre-requisites can be driven directly (Milestone 2). The provisioned flag
    /// lives inside the (leaked) temp dir.
    fn backend_with_admin() -> (Mutex<Backend>, User) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = AuthStore::open(&dir.path().join("auth.db")).expect("open auth store");
        let admin = store.create_user("admin@box", "Admin", "password1", UserRole::Admin).expect("admin");
        let backend = Backend::new(
            dir.path().to_path_buf(),
            100.0,
            PathBuf::from("/tmp/cp-maint-test-realms"),
            PathBuf::from("/tmp/cp-maint-test-bin"),
            Some(store),
            Duration::from_secs(3600),
        );
        std::mem::forget(dir);
        (Mutex::new(backend), admin)
    }

    /// Persist a valid identity through the real handler (Caddy reload is skipped
    /// in tests — no `CP_CADDYFILE`), so finalize's identity pre-req is met.
    fn set_test_identity(state: &Mutex<Backend>) {
        let body = br#"{"name":"pilot.acme.corp","ip":"192.168.1.116"}"#;
        assert_eq!(identity::set_identity(state, body).status, 200, "identity saved");
    }

    #[test]
    fn finalize_sets_provisioned_and_is_idempotent() {
        let (state, admin) = backend_with_admin();

        // Before finalize the box reports unprovisioned.
        assert!(status(&state).body.contains("\"provisioned\":false"), "fresh box is unprovisioned");

        // With the identity set and the password changed, finalize succeeds.
        set_test_identity(&state);
        let r = finalize(&state, Some(&admin));
        assert_eq!(r.status, 200, "finalize succeeds once pre-reqs are met");
        assert!(r.body.contains("\"provisioned\":true"));

        // Status now reflects the durable flag, and re-finalizing is idempotent.
        assert!(status(&state).body.contains("\"provisioned\":true"), "status reflects the flag");
        assert_eq!(finalize(&state, Some(&admin)).status, 200, "re-finalize is idempotent");
    }

    #[test]
    fn finalize_is_blocked_until_the_paper_password_is_changed() {
        let (state, admin) = backend_with_admin();
        set_test_identity(&state); // isolate the password gate from the identity gate
        // An admin who still must change the seeded paper password cannot finalize.
        let mut unchanged = admin.clone();
        unchanged.must_change_password = true;
        let r = finalize(&state, Some(&unchanged));
        assert_eq!(r.status, 412, "finalize refused until the password is changed");
        assert!(!status(&state).body.contains("\"provisioned\":true"), "box stays unprovisioned");
    }

    #[test]
    fn finalize_is_blocked_until_the_box_is_named() {
        let (state, admin) = backend_with_admin();
        // Password changed (default) but no identity yet → blocked.
        let r = finalize(&state, Some(&admin));
        assert_eq!(r.status, 412, "finalize refused until the box name/IP is set");
        assert!(!status(&state).body.contains("\"provisioned\":true"));
    }

    #[test]
    fn set_identity_validates_and_status_reflects_it() {
        let (state, _admin) = backend_with_admin();
        assert!(status(&state).body.contains("\"identity_set\":false"), "no identity initially");

        // Bad IP → 400.
        assert_eq!(identity::set_identity(&state, br#"{"name":"box","ip":"nope"}"#).status, 400);
        // Valid → 200, and status + GET reflect it.
        set_test_identity(&state);
        assert!(status(&state).body.contains("\"identity_set\":true"), "status flips after identity is set");
        let got = identity::get_identity(&state);
        assert!(got.body.contains("pilot.acme.corp") && got.body.contains("192.168.1.116"), "GET returns the identity");
    }

    #[test]
    fn admin_gate_enforces_role_on_protected_routes() {
        let (state, admin_tok, user_tok) = backend_with_users();
        let protected = ["api", "maint", "password"];

        // No token → 401.
        let no_token = authenticate(&state, &protected, None);
        assert_eq!(no_token.unwrap_err().status, 401, "missing token is 401");

        // Valid User-role token → 403.
        let as_user = authenticate(&state, &protected, Some(&user_tok));
        assert_eq!(as_user.unwrap_err().status, 403, "User role is forbidden");

        // Valid Admin-role token → 200 (Ok with the user).
        let as_admin = authenticate(&state, &protected, Some(&admin_tok)).expect("admin passes");
        assert_eq!(as_admin.expect("user present").role, UserRole::Admin);
    }

    #[test]
    fn public_routes_bypass_the_gate_even_without_a_token() {
        let (state, _admin, _user) = backend_with_users();
        assert!(authenticate(&state, &["api", "maint", "status"], None).expect("status public").is_none());
        assert!(authenticate(&state, &["api", "maint", "login"], None).expect("login public").is_none());
    }

    #[test]
    fn protected_route_without_auth_enabled_is_unavailable() {
        // Backend with auth disabled — the maintenance plane is meaningless.
        let dir = tempfile::tempdir().expect("tempdir");
        let backend = Backend::new(
            dir.path().to_path_buf(),
            100.0,
            PathBuf::from("/tmp/cp-maint-test-realms"),
            PathBuf::from("/tmp/cp-maint-test-bin"),
            None,
            Duration::from_secs(3600),
        );
        let state = Mutex::new(backend);
        let reply = authenticate(&state, &["api", "maint", "password"], Some("whatever")).unwrap_err();
        assert_eq!(reply.status, 503, "no auth → maintenance plane unavailable");
    }

    #[test]
    fn product_routes_are_absent_from_the_maintenance_router() {
        let (state, admin_tok, _user) = backend_with_users();
        // A core product route is a 404 on the maintenance router even for an admin.
        let reply = route(&Method::Get, &["api", "fleet"], &state, &[], Some(&admin_tok), None);
        assert_eq!(reply.status, 404, "product fleet route is not routed on the maintenance plane");
        let reply = route(&Method::Get, &["api", "agent", "x", "threads"], &state, &[], Some(&admin_tok), None);
        assert_eq!(reply.status, 404, "product agent route is not routed on the maintenance plane");
    }

    #[test]
    fn public_route_whitelist_is_minimal() {
        assert!(is_public_route(&["api", "maint", "status"]));
        assert!(is_public_route(&["api", "maint", "login"]));
        assert!(!is_public_route(&["api", "maint", "password"]));
        assert!(!is_public_route(&["api", "maint", "identity"]));
        assert!(!is_public_route(&["api", "maint", "me"]));
    }

    #[test]
    fn lan_predicate_accepts_local_refuses_public() {
        // Loopback + RFC-1918 + link-local are LAN.
        assert!(is_lan_addr(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(is_lan_addr(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 116))));
        assert!(is_lan_addr(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))));
        assert!(is_lan_addr(IpAddr::V4(Ipv4Addr::new(172, 16, 4, 9))));
        assert!(is_lan_addr(IpAddr::V4(Ipv4Addr::new(169, 254, 0, 1))));
        assert!(is_lan_addr(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(is_lan_addr("fd00::1".parse().unwrap()));
        assert!(is_lan_addr("fe80::1".parse().unwrap()));
        // Public addresses are refused.
        assert!(!is_lan_addr(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
        assert!(!is_lan_addr(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
        assert!(!is_lan_addr("2001:4860:4860::8888".parse().unwrap()));
    }
}
