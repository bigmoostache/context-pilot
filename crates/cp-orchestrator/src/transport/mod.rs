//! Frontend transport — the backend's HTTP face to the browser (design doc §9,
//! roadmap P7-P8).
//!
//! The transport is **REST + Server-Sent Events over `tiny_http`**: a blocking,
//! thread-per-connection HTTP server, the same shape as the rest of the backend
//! (no async runtime). REST serves loads, point queries, and non-streaming
//! actions; SSE pushes rev-numbered oplog deltas and ephemeral stream hints,
//! with reconnect-replay-by-`rev` provided natively by the SSE `Last-Event-ID`
//! mechanism (see [`sse`]).
//!
//! # Layers
//!
//! * [`Backend`] — the shared state the runtime loop owns and the handlers read
//!   (materialized view, stream hub, ticket store, agents dir),
//!   accessed under a single [`Mutex`].
//! * [`rest`] — request/response handlers returning a transport-agnostic
//!   [`HttpReply`](rest::HttpReply).
//! * [`stream::ticket`] — single-use SSE upgrade tickets (I9b).
//! * [`sse`] — the SSE encoder and blocking body reader.
//! * [`serve`] — the acceptor loop binding it all to a socket.
//!
//! Routes (all under `/api`):
//!
//! | Method | Path | Handler |
//! |---|---|---|
//! | `GET`  | `/api/fleet` | [`rest::fleet`] |
//! | `GET`  | `/api/agent/{id}` | [`rest::agent`] |
//! | `GET`  | `/api/agent/{id}/body/{hash}` | [`rest::body`] |
//! | `POST` | `/api/agent/{id}/command` | [`rest::command`] |
//! | `POST` | `/api/ticket` | [`rest::mint_ticket`] |
//! | `GET`  | `/api/stream?agent={id}&ticket={t}` | SSE (this module) |

mod auth;
mod files;
pub mod inspect;
pub mod it;
mod query;
pub mod rest;
// `pub` so the `sse` and `ticket` submodules it now contains stay as reachable
// as they were at the transport root (else their `pub` items trip
// `unreachable_pub`).
pub mod stream;

use std::io::Read as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use tiny_http::{Header, Method, Request, Response, Server};

use cp_wire::types::registry::Entry;

use query::QueryParams;

pub use rest::Backend;

/// Maximum request body size accepted on a POST route (32 MiB) — bounds memory
/// against a client that sends an endless stream, while comfortably fitting any
/// realistic chat message. A `SendMessage` command's body is the message text
/// wrapped in a small JSON envelope; the old 1 MiB cap silently truncated a
/// large paste (e.g. a big log or file), turning it into invalid JSON that the
/// command handler then rejected with `400` — the "big messages don't go
/// through" symptom (T274). 32 MiB allows ~32M characters, effectively no limit
/// for text, yet still a finite DoS guard. Kept in lockstep with the agent
/// intake's `MAX_CONNECTION_BUFFER` (the other cap on the same path).
const MAX_BODY: u64 = 32 * 1024 * 1024;

/// Bind an HTTP server to `addr` and serve the product cockpit until the process
/// exits. Back-compat shim over [`serve_bound`].
///
/// Each request runs on its own thread (`tiny_http`'s blocking model). A
/// streaming request occupies its thread for the lifetime of the connection;
/// everything else returns promptly.
///
/// # Errors
///
/// Returns an error string if the address cannot be bound.
pub fn serve(addr: &str, state: Arc<Mutex<Backend>>) -> Result<(), String> {
    let server = Server::http(addr).map_err(|e| e.to_string())?;
    serve_bound(server, state);
    Ok(())
}

/// Serve the product cockpit on an already-bound [`Server`], thread-per-request,
/// until the server is dropped.
///
/// Split out so a caller that needs the bound address up-front — notably a test
/// binding `127.0.0.1:0` to claim an ephemeral port — can read
/// [`Server::server_addr`] before handing the server here. There is a single
/// transport face now (design §13.4 removed the separate maintenance plane), so
/// this dispatches every request through the one product [`handle`] pipeline.
pub fn serve_bound(server: Server, state: Arc<Mutex<Backend>>) {
    for request in server.incoming_requests() {
        let state = Arc::clone(&state);
        let _handle = thread::spawn(move || handle(request, &state));
    }
}

/// Route one request: dispatch to a REST handler or the SSE stream.
fn handle(mut request: Request, state: &Arc<Mutex<Backend>>) {
    let (path, query) = split_url(request.url());
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let method = request.method().clone();

    // CORS preflight — return 204 with permissive headers.
    if method == Method::Options {
        let mut response = Response::from_string("").with_status_code(204);
        for header in cors_headers() {
            response = response.with_header(header);
        }
        let _sent = request.respond(response);
        return;
    }

    // Readiness probe (update-policy §5.2/§5.5) — top-level (not `/api`),
    // unauthenticated, loopback-only. Served before the SPA fallback and the
    // auth gate: its consumer is the box itself (the health-gated boot commit
    // and the systemd-era rollback machinery), which must reach it before any
    // session can exist. Non-loopback callers get a flat 403.
    if method == Method::Get && segments.as_slice() == ["healthz"] {
        let reply = if request.remote_addr().is_some_and(|a| a.ip().is_loopback()) {
            it::health::healthz(state)
        } else {
            rest::HttpReply::error(403, "loopback only")
        };
        respond_json(request, &reply);
        return;
    }

    // Static SPA serving (P-native): when `CP_WEB_ROOT` is set, every non-`/api`
    // GET is the web UI — served straight from disk with an index.html fallback
    // for client-side routes, BEFORE the auth gate (the shell + assets must load
    // for an unauthenticated visitor to even reach the login screen). API, SSE
    // and download routes all live under `/api`, so they are untouched.
    if method == Method::Get && segments.first() != Some(&"api") && files::web_root().is_some() {
        files::serve_static(request, &path);
        return;
    }

    // Extract the Bearer token for auth-aware handlers.
    let auth_token = request.headers().iter().find_map(|h| {
        if h.field.equiv("Authorization") { h.value.as_str().strip_prefix("Bearer ").map(str::to_owned) } else { None }
    });

    // Centralised auth gate (Phase 5, NFR-16). Validates the session for
    // protected routes when auth is enabled; no-op when disabled (NFR-09).
    let auth_user = match auth::authenticate(&state, &segments, auth_token.as_deref()) {
        Ok(user) => user,
        Err(reply) => {
            respond_json(request, &reply);
            return;
        }
    };

    // Per-agent ACL check (Phase 6). When auth is enabled and the route
    // targets a specific agent, verify the caller has access. System admins
    // bypass (FR-09); regular users need an ACL entry (FR-10).
    if let Some(agent_id) = auth::extract_agent_id(&segments) {
        if let Some(ref user) = auth_user {
            if !auth::authorize_agent(state, agent_id, user) {
                respond_json(request, &rest::HttpReply::error(403, "no access to this agent"));
                return;
            }
        }
    }

    // SSE stream is the one route that takes ownership of the request to stream.
    if method == Method::Get && segments.as_slice() == ["api", "stream"] {
        handle_stream(request, state, &query);
        return;
    }

    // File download — returns raw bytes, not JSON.
    if method == Method::Get {
        if let ["api", "agent", id, "avatar"] = segments.as_slice() {
            files::handle_avatar(request, state, id);
            return;
        }
        if let ["api", "agent", id, "fs", "download"] = segments.as_slice() {
            files::handle_download(request, state, id, &query);
            return;
        }
        if let ["api", "agent", id, "fs", "raw"] = segments.as_slice() {
            files::handle_raw(request, state, id, &query);
            return;
        }
        // IT: private-CA root download (design §13.5, re-homed from the maint plane).
        // Owns the `Request` for its non-JSON content type, so it can't route
        // through `route_rest`. Gate on `can_manage_it` here (a `None` caller is
        // god-mode, FR-v3-08); then reuse the maintenance handler verbatim.
        if let ["api", "it", "ca.crt"] = segments.as_slice() {
            if auth_user.as_ref().is_some_and(|u| !u.can_manage_it()) {
                respond_json(request, &rest::HttpReply::error(403, "IT management access required"));
            } else {
                it::ca::serve_ca_cert(request);
            }
            return;
        }
    }

    // Read the body up-front (only POST routes consume it). The mutable borrow
    // ends here, before the request is moved into the response.
    let body_bytes = if method == Method::Post || method == Method::Patch || method == Method::Put {
        read_body(&mut request)
    } else {
        Vec::new()
    };

    let reply =
        route_rest(&method, &segments, state, body_bytes.as_slice(), &query, auth_token.as_deref(), auth_user.as_ref());
    respond_json(request, &reply);
}

/// Read a request body fully into a buffer, bounded by [`MAX_BODY`].
fn read_body(request: &mut Request) -> Vec<u8> {
    let mut buf = Vec::new();
    let _read = request.as_reader().take(MAX_BODY).read_to_end(&mut buf);
    buf
}

/// Dispatch a non-streaming REST route to its handler.
fn route_rest(
    method: &Method,
    segments: &[&str],
    state: &Arc<Mutex<Backend>>,
    body_bytes: &[u8],
    query: &str,
    auth_token: Option<&str>,
    auth_user: Option<&crate::services::auth::types::User>,
) -> rest::HttpReply {
    match (method, segments) {
        (Method::Get, ["api", "health"]) => rest::HttpReply { status: 200, body: "{\"status\":\"ok\"}".to_owned() },
        (Method::Get, ["api", "providers"]) => inspect::providers::providers(query),

        // ── Auth routes (§6 of design doc) ──────────────────────────
        (Method::Get, ["api", "auth", "status"]) => auth::auth_status(state),
        (Method::Post, ["api", "auth", "login"]) => auth::login(state, body_bytes),
        (Method::Post, ["api", "auth", "register"]) => auth::register(state, body_bytes, auth_user),
        (Method::Post, ["api", "auth", "logout"]) => auth::logout(state, auth_token),
        (Method::Get, ["api", "auth", "me"]) => auth::me(state, auth_user),
        (Method::Patch, ["api", "auth", "me"]) => auth::update_me(state, body_bytes, auth_user),
        (Method::Post, ["api", "auth", "password"]) => auth::change_password(state, body_bytes, auth_user),
        (Method::Get, ["api", "auth", "sessions"]) => auth::list_sessions(state, auth_token, auth_user),
        (Method::Delete, ["api", "auth", "sessions", sid]) => auth::revoke_session(state, sid, auth_user),
        (Method::Get, ["api", "settings"]) => rest::get_settings(state, auth_user),
        (Method::Post, ["api", "settings"]) => rest::update_settings(state, body_bytes, auth_user),
        (Method::Get, ["api", "auth", "users"]) => auth::list_users(state, auth_user),
        (Method::Post, ["api", "auth", "users"]) => auth::create_user(state, body_bytes, auth_user),
        (Method::Delete, ["api", "auth", "users", user_id]) => auth::delete_user(state, user_id, auth_user),
        (Method::Post, ["api", "auth", "users", user_id, "logout"]) => {
            auth::force_logout_user(state, user_id, auth_user)
        }

        // ── ACL routes (Phase 6, §6 of design doc) ─────────────────
        (Method::Get, ["api", "agent", id, "acl"]) => auth::acl_list(state, id, auth_user),
        (Method::Post, ["api", "agent", id, "acl"]) => auth::acl_grant(state, id, body_bytes, auth_user),
        (Method::Patch, ["api", "agent", id, "acl", user_id]) => {
            auth::acl_update_role(state, id, user_id, body_bytes, auth_user)
        }
        (Method::Delete, ["api", "agent", id, "acl", user_id]) => auth::acl_revoke(state, id, user_id, auth_user),

        // ── Fleet + agent routes ────────────────────────────────────
        (Method::Get, ["api", "fleet"]) => rest::fleet(state, auth_user),
        (Method::Get, ["api", "fleet", "meta"]) => inspect::meta::fleet_meta(state, auth_user),
        (Method::Get, ["api", "fleet", "retired"]) => inspect::meta::fleet_retired(state, auth_user),
        (Method::Get, ["api", "metrics"]) => inspect::metrics::fleet_metrics(state, auth_user),

        // ── Env-key inspection (T399) + editing (T404) ────────────
        (Method::Get, ["api", "env-keys"]) => rest::env_keys_list(),
        (Method::Get, ["api", "env-keys", name]) => rest::env_key_reveal(name, auth_user),
        (Method::Put, ["api", "env-keys", name]) => {
            let body = String::from_utf8_lossy(body_bytes);
            rest::env_key_update(name, auth_user, &body)
        }

        // ── Vault snapshot (BridgeVault cache warm-up) ──────────────
        (Method::Get, ["api", "vault", "snapshot"]) => rest::vault_snapshot(auth_user),

        // ── IT infra (design §13.5, re-homed from the maint plane; can_manage_it) ──
        (Method::Get, ["api", "it", "ca", "fingerprint"]) => rest::it_ca_fingerprint(auth_user),
        (Method::Get, ["api", "it", "identity"]) => rest::it_get_identity(state, auth_user),
        (Method::Post, ["api", "it", "identity"]) => rest::it_set_identity(state, body_bytes, auth_user),
        (Method::Get, ["api", "it", "provisioned"]) => rest::it_provisioned(state, auth_user),

        (Method::Get, ["api", "agent", id]) => rest::agent(state, id),
        (Method::Get, ["api", "agent", id, "meta"]) => inspect::meta::agent_meta(state, id),
        (Method::Get, ["api", "agent", id, "metrics"]) => inspect::metrics::agent_metrics(state, id),
        (Method::Get, ["api", "agent", id, "vitals"]) => inspect::vitals::agent_vitals(state, id),
        (Method::Get, ["api", "agent", id, "body", hash]) => rest::body(state, id, hash),
        (Method::Get, ["api", "agent", id, "threads"]) => rest::threads(state, id),
        (Method::Get, ["api", "agent", id, "usage"]) => inspect::panels::usage(state, id, query),
        (Method::Get, ["api", "agent", id, "library"]) => inspect::panels::library(state, id),
        (Method::Get, ["api", "agent", id, "fs"]) => inspect::finder::fs_list(state, id, query),
        (Method::Get, ["api", "agent", id, "fs", "preview"]) => inspect::finder::fs_preview(state, id, query),
        (Method::Get, ["api", "agent", id, "fs", "sheet"]) => inspect::finder::fs_sheet(state, id, query),
        (Method::Get, ["api", "agent", id, "fs", "descriptions"]) => inspect::finder::fs_descriptions(state, id),
        (Method::Get, ["api", "agent", id, "conversation"]) => inspect::finder::conversation(state, id),
        (Method::Post, ["api", "agent", id, "command"]) => rest::command(state, id, body_bytes),
        (Method::Post, ["api", "agent", id, "library", "command"]) => rest::create_command(state, id, body_bytes),
        (Method::Post, ["api", "agent", id, "fs", "upload"]) => {
            inspect::finder::fs_upload(state, id, query, body_bytes)
        }
        (Method::Post, ["api", "agent", id, "fs", "upload-unique"]) => {
            inspect::finder::fs_upload_unique(state, id, query, body_bytes)
        }
        (Method::Post, ["api", "agent", id, "fs", "write"]) => inspect::finder::fs_write(state, id, query, body_bytes),
        (Method::Post, ["api", "agent", id, "fs", "mkdir"]) => inspect::finder::fs_mkdir(state, id, query),
        (Method::Post, ["api", "agent", id, "fs", "rename"]) => inspect::finder::fs_rename(state, id, query),
        (Method::Post, ["api", "agent", id, "fs", "move"]) => inspect::finder::fs_move(state, id, body_bytes),
        (Method::Post, ["api", "agent", id, "fs", "trash"]) => inspect::finder::fs_trash(state, id, body_bytes),
        (Method::Post, ["api", "agent", id, "restart"]) => rest::restart_agent(state, id),
        (Method::Post, ["api", "agent", id, "retire"]) => rest::retire_agent(state, id),
        (Method::Post, ["api", "agent", id, "unretire"]) => rest::unretire_agent(state, id),
        (Method::Post, ["api", "agent", id, "rename"]) => rest::rename_agent(state, id, body_bytes),
        (Method::Post, ["api", "agent", id, "avatar"]) => rest::upload_avatar(state, id, body_bytes),
        (Method::Delete, ["api", "agent", id, "avatar"]) => rest::delete_avatar(state, id),
        (Method::Post, ["api", "fleet", "create"]) => rest::create_agent(state, body_bytes, auth_user),
        (Method::Post, ["api", "ticket"]) => rest::mint_ticket(state, auth_user),

        // ── Release management (T427) — IT-management surface ──────
        // One guard for every `/api/releases/*` arm below (update-policy §1
        // problem 2): a real caller without `can_manage_it` (Admin+) is
        // refused here; a `None` caller means access control is off →
        // god-mode passes through (design §13.10).
        (_, ["api", "releases", ..]) if auth_user.is_some_and(|u| !u.can_manage_it()) => {
            rest::HttpReply::error(403, "IT management access required")
        }
        (Method::Get, ["api", "releases"]) => rest::list_releases(state),
        (Method::Put, ["api", "releases", "arch"]) => rest::set_arch(state, body_bytes),
        (Method::Post, ["api", "releases", "download"]) => rest::download_release(state, body_bytes),
        (Method::Put, ["api", "releases", "select"]) => rest::select_release(state, body_bytes),
        (Method::Post, ["api", "releases", "deploy"]) => rest::deploy_fleet(state, body_bytes),
        (Method::Post, ["api", "releases", "restart-orchestrator"]) => rest::restart_orchestrator(state),
        (Method::Delete, ["api", "releases", tag]) => rest::delete_release(state, tag),

        // ── Claude Code usage + login (OAuth) ───────────────────────
        (Method::Get, ["api", "claude-usage"]) => rest::claude_usage(),
        (Method::Get, ["api", "claude-login", "status"]) => rest::token_status(),
        (Method::Post, ["api", "claude-login", "start"]) => rest::login_start(state),
        (Method::Post, ["api", "claude-login", "complete"]) => rest::login_complete(state, body_bytes),
        (Method::Post, ["api", "claude-login", "refresh"]) => rest::refresh_login(),

        // ── Claude multi-account token vault ────────────────────────
        (Method::Get, ["api", "claude-accounts"]) => rest::list_accounts(),
        (Method::Post, ["api", "claude-accounts", "store"]) => rest::store_account(),
        (Method::Post, ["api", "claude-accounts", "switch"]) => rest::switch_account(body_bytes),
        (Method::Delete, ["api", "claude-accounts", email]) => rest::delete_account(email),

        _ => rest::HttpReply { status: 404, body: "{\"error\":\"not found\"}".to_owned() },
    }
}

/// Redeem the ticket and stream an agent's deltas as SSE until disconnect.
fn handle_stream(request: Request, state: &Arc<Mutex<Backend>>, query: &str) {
    let params = QueryParams::parse(query);
    let Some(agent_id) = params.get("agent") else {
        respond_json(request, &rest::HttpReply { status: 400, body: "{\"error\":\"missing agent\"}".to_owned() });
        return;
    };
    let Some(token) = params.get("ticket") else {
        respond_json(request, &rest::HttpReply { status: 401, body: "{\"error\":\"missing ticket\"}".to_owned() });
        return;
    };

    // Single-use ticket redemption (Phase 7: now returns user identity).
    let ticket = state.lock().ok().and_then(|mut b| b.tickets.redeem(token));
    let Some(ticket) = ticket else {
        respond_json(request, &rest::HttpReply { status: 401, body: "{\"error\":\"invalid ticket\"}".to_owned() });
        return;
    };

    // Phase 7: per-agent ACL check on SSE connect. The ticket carries the
    // minting user's identity; when auth is enabled we verify they have access
    // to the requested agent before committing to a stream. System admins
    // bypass (FR-09). When auth is disabled (user_id is None) the check is
    // skipped entirely (NFR-09).
    if let Some(ref user_id) = ticket.user_id {
        let authorized = state.lock().ok().map_or(false, |b| {
            match b.auth.as_ref() {
                Some(auth) => match auth.get_user_by_id(user_id) {
                    Ok(Some(user)) => {
                        // Implicit access to all agents (manager+) bypasses the
                        // per-agent ACL (design §13.3); everyone else needs a row.
                        if user.can_manage_all_agents() {
                            true
                        } else {
                            auth.check_access(agent_id, user_id).map(|role| role.is_some()).unwrap_or(false)
                        }
                    }
                    _ => false,
                },
                None => true, // auth not enabled — pass through
            }
        });
        if !authorized {
            respond_json(
                request,
                &rest::HttpReply { status: 403, body: "{\"error\":\"no access to this agent\"}".to_owned() },
            );
            return;
        }
    }

    // Resolve the agent's oplog directory before committing to a stream.
    let Some(entry) = load_entry(state, agent_id) else {
        respond_json(request, &rest::HttpReply { status: 404, body: "{\"error\":\"unknown agent\"}".to_owned() });
        return;
    };

    let last_rev = last_event_id(&request).or_else(|| params.get("last_rev").and_then(|s| s.parse().ok()));

    let (sink, body) = stream::sse::channel();
    let producer_state = Arc::clone(state);
    let agent = agent_id.to_owned();
    let oplog_dir = PathBuf::from(&entry.oplog_path);
    let _producer = thread::spawn(move || stream::run_stream(&sink, &producer_state, &agent, &oplog_dir, last_rev));

    stream::sse::stream_to_client(request, body);
}

/// Load an agent's registry record from the backend's agents directory.
fn load_entry(state: &Arc<Mutex<Backend>>, id: &str) -> Option<Entry> {
    let dir = state.lock().ok()?.agents_dir.clone();
    let raw = std::fs::read(dir.join(format!("{id}.json"))).ok()?;
    serde_json::from_slice::<Entry>(&raw).ok()
}

/// CORS response headers permitting the Vite dev server (or any origin) to
/// call the backend. Tighten to a specific origin in production if needed.
fn cors_headers() -> Vec<Header> {
    [
        Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]),
        Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, PUT, PATCH, DELETE, OPTIONS"[..]),
        Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type, Last-Event-ID, Authorization"[..]),
        // Expose Content-Disposition so cross-origin fetch() (web dev server →
        // backend) can read the server-chosen download filename. Without this,
        // the header is hidden by CORS and the client falls back to the URL's
        // last path segment — a folder download then saves as "src" instead of
        // the "src.zip" the backend actually sends.
        Header::from_bytes(&b"Access-Control-Expose-Headers"[..], &b"Content-Disposition"[..]),
    ]
    .into_iter()
    .filter_map(Result::ok)
    .collect()
}

/// Respond with a JSON [`HttpReply`](rest::HttpReply), including CORS headers.
fn respond_json(request: Request, reply: &rest::HttpReply) {
    let mut response = Response::from_string(&reply.body).with_status_code(reply.status);
    if let Ok(header) = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]) {
        response = response.with_header(header);
    }
    for header in cors_headers() {
        response = response.with_header(header);
    }
    let _sent = request.respond(response);
}

/// Extract a `Last-Event-ID` header value as a `rev`.
fn last_event_id(request: &Request) -> Option<u64> {
    request.headers().iter().find(|h| h.field.equiv("Last-Event-ID")).and_then(|h| h.value.as_str().parse().ok())
}

/// Split a URL into its path and query-string halves.
fn split_url(url: &str) -> (String, String) {
    match url.split_once('?') {
        Some((path, query)) => (path.to_owned(), query.to_owned()),
        None => (url.to_owned(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_url_separates_path_and_query() {
        assert_eq!(
            split_url("/api/stream?agent=a1&ticket=x"),
            ("/api/stream".to_owned(), "agent=a1&ticket=x".to_owned())
        );
        assert_eq!(split_url("/api/fleet"), ("/api/fleet".to_owned(), String::new()));
    }
}
