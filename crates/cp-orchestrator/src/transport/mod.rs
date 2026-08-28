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
//! | `POST` | `/api/agent/{id}/command` | [`rest::command`] |
//! | `GET`  | `/api/stream?agent={id}&ticket={t}` | SSE (this module) |

mod auth;
mod files;
pub mod inspect;
pub mod it;
pub mod rest;
// `pub` so the `sse` and `ticket` submodules it now contains stay as reachable
// as they were at the transport root (else their `pub` items trip
// `unreachable_pub`).
pub mod stream;

/// The non-streaming REST route table + raw-bytes GET dispatcher, split out to
/// keep this file within the 500-line budget.
mod router;

use std::io::Read as _;
use std::sync::{Arc, Mutex};
use std::thread;

use tiny_http::{Header, Method, Request, Response, Server};

pub use rest::{Backend, BackendPaths};

/// Maximum request body size accepted on a POST route (32 MiB) — bounds memory
/// against a client that sends an endless stream, while comfortably fitting any
/// realistic chat message. A `SendMessage` command's body is the message text
/// wrapped in a small JSON envelope; the old 1 MiB cap silently truncated a
/// large paste (e.g. a big log or file), turning it into invalid JSON that the
/// command handler then rejected with `400` — the "big messages don't go
/// through" symptom (T274). 32 MiB allows ~32M characters, effectively no limit
/// for text, yet still a finite `DoS` guard. Kept in lockstep with the agent
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
pub fn serve(addr: &str, state: &Arc<Mutex<Backend>>) -> Result<(), String> {
    let server = Server::http(addr).map_err(|e| e.to_string())?;
    serve_bound(&server, state);
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
pub fn serve_bound(server: &Server, state: &Arc<Mutex<Backend>>) {
    for request in server.incoming_requests() {
        let handler_state = Arc::clone(state);
        let _handle = thread::spawn(move || handle(request, &handler_state));
    }
}

/// Route one request: dispatch to a REST handler or the SSE stream.
fn handle(mut request: Request, state: &Arc<Mutex<Backend>>) {
    let (path, query) = split_url(request.url());
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let method = request.method().clone();

    // CORS preflight — return 204 with permissive headers.
    if method == Method::Options {
        respond_preflight(request);
        return;
    }

    // Readiness probe (update-policy §5.2/§5.5) — top-level (not `/api`),
    // unauthenticated, loopback-only. Served before the SPA fallback and the
    // auth gate: its consumer is the box itself (the health-gated boot commit
    // and the systemd-era rollback machinery), which must reach it before any
    // session can exist. Non-loopback callers get a flat 403.
    if method == Method::Get && segments.as_slice() == ["healthz"] {
        respond_healthz(request, state);
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
    let auth_token = bearer_token(&request);

    // Centralised auth gate (Phase 5, NFR-16). Validates the session for
    // protected routes when auth is enabled; no-op when disabled (NFR-09).
    let auth_user = match auth::authenticate(state, &segments, auth_token.as_deref()) {
        Ok(user) => user,
        Err(reply) => {
            respond_json(request, &reply);
            return;
        }
    };

    // Per-agent ACL check (Phase 6). When auth is enabled and the route
    // targets a specific agent, verify the caller has access. System admins
    // bypass (FR-09); regular users need an ACL entry (FR-10).
    if let Some(agent_id) = auth::extract_agent_id(&segments)
        && let Some(user) = auth_user.as_ref()
        && !auth::authorize_agent(state, agent_id, user)
    {
        respond_json(request, &rest::HttpReply::error(403, "no access to this agent"));
        return;
    }

    // SSE stream is the one route that takes ownership of the request to stream.
    if method == Method::Get && segments.as_slice() == ["api", "stream"] {
        stream::upgrade::handle_stream(request, state, &query);
        return;
    }

    // File download — returns raw bytes, not JSON. Delegated to [`try_raw_route`]
    // (a GET-only dispatcher that owns the `Request` for its non-JSON body):
    // `None` means it handled and consumed the request; `Some(request)` gives it
    // back for the rest of the pipeline.
    if method == Method::Get {
        let get_ctx = RouteCtx {
            state,
            body_bytes: &[],
            query: &query,
            auth_token: auth_token.as_deref(),
            auth_user: auth_user.as_ref(),
        };
        match router::try_raw_route(request, &segments, get_ctx) {
            Some(returned) => request = returned,
            None => return,
        }
    }

    // Read the body up-front (only POST routes consume it). The mutable borrow
    // ends here, before the request is moved into the response.
    let body_bytes = if method == Method::Post || method == Method::Patch || method == Method::Put {
        read_body(&mut request)
    } else {
        Vec::new()
    };

    let reply = router::route_rest(
        &method,
        &segments,
        RouteCtx {
            state,
            body_bytes: body_bytes.as_slice(),
            query: &query,
            auth_token: auth_token.as_deref(),
            auth_user: auth_user.as_ref(),
        },
    );
    respond_json(request, &reply);
}

/// Read a request body fully into a buffer, bounded by [`MAX_BODY`].
fn read_body(request: &mut Request) -> Vec<u8> {
    let mut buf = Vec::new();
    let _read = request.as_reader().take(MAX_BODY).read_to_end(&mut buf);
    buf
}

/// Extract the `Authorization: Bearer <token>` value, if present.
fn bearer_token(request: &Request) -> Option<String> {
    request.headers().iter().find_map(|h| {
        if h.field.equiv("Authorization") { h.value.as_str().strip_prefix("Bearer ").map(str::to_owned) } else { None }
    })
}

/// Answer a CORS preflight with `204 No Content` and the permissive CORS headers.
fn respond_preflight(request: Request) {
    let mut response = Response::from_string("").with_status_code(204i32);
    for header in cors_headers() {
        response = response.with_header(header);
    }
    let _sent = request.respond(response);
}

/// Answer the loopback-only readiness probe (`GET /healthz`): the box's own
/// health check, served before the auth gate. Non-loopback callers get `403`.
fn respond_healthz(request: Request, state: &Arc<Mutex<Backend>>) {
    let reply = if request.remote_addr().is_some_and(|a| a.ip().is_loopback()) {
        it::health::healthz(state)
    } else {
        rest::HttpReply::error(403, "loopback only")
    };
    respond_json(request, &reply);
}

/// Borrowed per-request routing context — the shared inputs every REST handler
/// draws on, bundled so [`route_rest`] stays under the argument-count cap. The
/// match arms destructure it once at the top and reference the locals verbatim.
#[derive(Clone, Copy)]
struct RouteCtx<'ctx> {
    /// The shared backend state.
    state: &'ctx Arc<Mutex<Backend>>,
    /// The request body (empty for non-POST/PUT/PATCH routes).
    body_bytes: &'ctx [u8],
    /// The raw query string.
    query: &'ctx str,
    /// The caller's Bearer token, if present.
    auth_token: Option<&'ctx str>,
    /// The authenticated user, when auth is enabled.
    auth_user: Option<&'ctx crate::services::auth::types::User>,
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
