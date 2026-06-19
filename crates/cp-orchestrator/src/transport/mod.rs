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
//!   (materialized view, cost breaker, stream hub, ticket store, agents dir),
//!   accessed under a single [`Mutex`].
//! * [`rest`] — request/response handlers returning a transport-agnostic
//!   [`HttpReply`](rest::HttpReply).
//! * [`ticket`] — single-use SSE upgrade tickets (I9b).
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

pub mod inspect;
mod query;
pub mod rest;
pub mod sse;
mod stream;
pub mod ticket;

use std::collections::HashSet;
use std::io::Read as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;


use tiny_http::{Header, Method, Request, Response, Server};

use cp_wire::types::registry::Entry;


use crate::inspect::StateReader;
use crate::services::{CostBreaker, MaterializedView, StreamHub};
use crate::supervisor::AgentSupervisor;
use ticket::TicketStore;
use query::QueryParams;

/// Default per-agent SSE subscriber buffer capacity.
const DEFAULT_SUB_CAPACITY: usize = 256;

/// Maximum request body size accepted on a POST route (1 MiB) — bounds memory
/// against a client that sends an endless stream.
const MAX_BODY: u64 = 1024 * 1024;

/// Shared backend state read by transport handlers and written by the runtime
/// loop.
///
/// Wrapped in an [`Arc<Mutex<Backend>>`](Mutex) for the thread-per-connection
/// server. Handlers hold the lock only briefly and never across blocking agent
/// I/O.
#[derive(Debug)]
pub struct Backend {
    /// Per-agent projected fleet state.
    pub(crate) view: MaterializedView,
    /// Durable per-agent spend breaker.
    pub(crate) breaker: CostBreaker,
    /// Per-agent ephemeral stream fan-out.
    pub(crate) hub: StreamHub,
    /// Single-use SSE upgrade tickets.
    pub(crate) tickets: TicketStore,
    /// Read-only, mtime-cached reader of agent persistence files.
    pub(crate) inspect: StateReader,
    /// Directory of agent registry records (`<id>.json`).
    pub(crate) agents_dir: PathBuf,
    /// Agents whose tier-② state has changed since the last SSE sweep.
    /// SSE producers drain this per-agent to emit `invalidate` events.
    pub(crate) dirty_agents: HashSet<String>,
    /// Process-lifecycle manager — spawns dashboard-created agents (PTY) under
    /// a binary allow-list (R2-15).
    pub(crate) supervisor: AgentSupervisor,
    /// Root directory new agents' realm folders are created under.
    pub(crate) agents_root: PathBuf,
    /// The `cp` TUI binary the supervisor spawns (also the sole allow-list
    /// entry).
    pub(crate) agent_binary: PathBuf,
}

impl Backend {
    /// Build a backend with empty services and the given per-agent cost budget.
    ///
    /// `agents_root` is where dashboard-created agents' folders are made, and
    /// `agent_binary` is the `cp` TUI binary the supervisor may spawn — it
    /// seeds the supervisor's allow-list (R2-15), so it is the only binary that
    /// can ever be launched.
    #[must_use]
    pub fn new(
        agents_dir: PathBuf,
        budget_usd: f64,
        agents_root: PathBuf,
        agent_binary: PathBuf,
    ) -> Self {
        Self {
            view: MaterializedView::new(),
            breaker: CostBreaker::new(budget_usd),
            hub: StreamHub::new(DEFAULT_SUB_CAPACITY),
            tickets: TicketStore::new(),
            inspect: StateReader::new(),
            agents_dir,
            dirty_agents: HashSet::new(),
            supervisor: AgentSupervisor::new(&[agent_binary.clone()]),
            agents_root,
            agent_binary,
        }
    }

    /// Mutable access to the materialized view (for the runtime loop's fold).
    pub fn view_mut(&mut self) -> &mut MaterializedView {
        &mut self.view
    }

    /// Mutable access to the cost breaker (for the runtime loop's observe).
    pub fn breaker_mut(&mut self) -> &mut CostBreaker {
        &mut self.breaker
    }

    /// Mutable access to the stream hub (for the runtime loop's publish).
    pub fn hub_mut(&mut self) -> &mut StreamHub {
        &mut self.hub
    }

    /// Mutable access to the state reader (for inspection endpoints).
    pub fn inspect_mut(&mut self) -> &mut StateReader {
        &mut self.inspect
    }

    /// Mark an agent's state as dirty — SSE producers will emit an
    /// `invalidate` event on the next sweep.
    pub fn mark_dirty(&mut self, agent_id: &str) {
        let _new = self.dirty_agents.insert(agent_id.to_owned());
    }

    /// Check and clear the dirty flag for an agent. Returns `true` if the
    /// agent was dirty (the caller should emit an `invalidate` SSE event).
    pub fn take_dirty(&mut self, agent_id: &str) -> bool {
        self.dirty_agents.remove(agent_id)
    }

    /// Construct a backend from explicit services — used by tests.
    #[cfg(test)]
    pub(crate) fn for_test(agents_dir: PathBuf, view: MaterializedView, breaker: CostBreaker) -> Self {
        Self { view, breaker, hub: StreamHub::new(DEFAULT_SUB_CAPACITY), tickets: TicketStore::new(), inspect: StateReader::new(), agents_dir, dirty_agents: HashSet::new(), supervisor: AgentSupervisor::new(&[]), agents_root: PathBuf::from("/tmp/cp-test-realms"), agent_binary: PathBuf::from("/tmp/cp-test-bin") }
    }
}

/// Bind an HTTP server to `addr` and serve transport requests until the process
/// exits.
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

/// Serve transport requests on an already-bound [`Server`], thread-per-request,
/// until the server is dropped.
///
/// Split out of [`serve`] so a caller that needs the bound address up-front —
/// notably an integration test binding `127.0.0.1:0` to claim an ephemeral
/// port — can read [`Server::server_addr`] before handing the server here.
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

    // SSE stream is the one route that takes ownership of the request to stream.
    if method == Method::Get && segments.as_slice() == ["api", "stream"] {
        handle_stream(request, state, &query);
        return;
    }

    // File download — returns raw bytes, not JSON.
    if method == Method::Get {
        if let ["api", "agent", id, "fs", "download"] = segments.as_slice() {
            handle_download(request, state, id, &query);
            return;
        }
    }

    // Read the body up-front (only POST routes consume it). The mutable borrow
    // ends here, before the request is moved into the response.
    let body_bytes = if method == Method::Post { read_body(&mut request) } else { Vec::new() };

    let reply = route_rest(&method, &segments, state, body_bytes.as_slice(), &query);
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
) -> rest::HttpReply {
    match (method, segments) {
        (Method::Get, ["api", "health"]) => rest::HttpReply { status: 200, body: "{\"status\":\"ok\"}".to_owned() },
        (Method::Get, ["api", "fleet"]) => rest::fleet(state),
        (Method::Get, ["api", "fleet", "meta"]) => inspect::meta::fleet_meta(state),
        (Method::Get, ["api", "metrics"]) => inspect::metrics::fleet_metrics(state),
        (Method::Get, ["api", "agent", id]) => rest::agent(state, id),
        (Method::Get, ["api", "agent", id, "meta"]) => inspect::meta::agent_meta(state, id),
        (Method::Get, ["api", "agent", id, "metrics"]) => inspect::metrics::agent_metrics(state, id),
        (Method::Get, ["api", "agent", id, "vitals"]) => inspect::vitals::agent_vitals(state, id),
        (Method::Get, ["api", "agent", id, "body", hash]) => rest::body(state, id, hash),
        (Method::Get, ["api", "agent", id, "threads"]) => rest::threads(state, id),
        (Method::Get, ["api", "agent", id, "panels"]) => inspect::panels::panel_list(state, id),
        (Method::Get, ["api", "agent", id, "memory"]) => inspect::panels::memory(state, id),
        (Method::Get, ["api", "agent", id, "todos"]) => inspect::panels::todos(state, id, query),
        (Method::Get, ["api", "agent", id, "spine"]) => inspect::panels::spine(state, id, query),
        (Method::Get, ["api", "agent", id, "queue"]) => inspect::panels::queue(state, id, query),
        (Method::Get, ["api", "agent", id, "scratchpad"]) => inspect::panels::scratchpad(state, id, query),
        (Method::Get, ["api", "agent", id, "tree"]) => inspect::panels::tree(state, id),
        (Method::Get, ["api", "agent", id, "callbacks"]) => inspect::panels::callbacks(state, id),
        (Method::Get, ["api", "agent", id, "tools"]) => inspect::panels::tools(state, id),
        (Method::Get, ["api", "agent", id, "radar"]) => inspect::panels::radar(state, id),
        (Method::Get, ["api", "agent", id, "entities"]) => inspect::panels::entities(state, id),
        (Method::Get, ["api", "agent", id, "usage"]) => inspect::panels::usage(state, id, query),
        (Method::Get, ["api", "agent", id, "library"]) => inspect::panels::library(state, id),
        (Method::Get, ["api", "agent", id, "fs"]) => inspect::finder::fs_list(state, id, query),
        (Method::Get, ["api", "agent", id, "fs", "preview"]) => inspect::finder::fs_preview(state, id, query),
        (Method::Get, ["api", "agent", id, "conversation"]) => inspect::finder::conversation(state, id),
        (Method::Post, ["api", "agent", id, "command"]) => rest::command(state, id, body_bytes),
        (Method::Post, ["api", "agent", id, "restart"]) => rest::restart_agent(state, id),
        (Method::Post, ["api", "fleet", "create"]) => rest::create_agent(state, body_bytes),
        (Method::Post, ["api", "ticket"]) => rest::mint_ticket(state),
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

    // Single-use ticket redemption.
    let redeemed = state.lock().map(|mut b| b.tickets.redeem(token)).unwrap_or(false);
    if !redeemed {
        respond_json(request, &rest::HttpReply { status: 401, body: "{\"error\":\"invalid ticket\"}".to_owned() });
        return;
    }

    // Resolve the agent's oplog directory before committing to a stream.
    let Some(entry) = load_entry(state, agent_id) else {
        respond_json(request, &rest::HttpReply { status: 404, body: "{\"error\":\"unknown agent\"}".to_owned() });
        return;
    };

    let last_rev = last_event_id(&request).or_else(|| params.get("last_rev").and_then(|s| s.parse().ok()));

    let (sink, body) = sse::channel();
    let producer_state = Arc::clone(state);
    let agent = agent_id.to_owned();
    let oplog_dir = PathBuf::from(&entry.oplog_path);
    let _producer = thread::spawn(move || stream::run_stream(&sink, &producer_state, &agent, &oplog_dir, last_rev));

    sse::stream_to_client(request, body);
}

/// Serve a raw file download with `Content-Disposition: attachment`.
fn handle_download(request: Request, state: &Arc<Mutex<Backend>>, id: &str, query: &str) {
    match inspect::finder::fs_download(state, id, query) {
        Ok((bytes, filename)) => {
            let mut response = Response::from_data(bytes).with_status_code(200);
            if let Ok(h) = Header::from_bytes(
                &b"Content-Disposition"[..],
                format!("attachment; filename=\"{filename}\"").as_bytes(),
            ) {
                response = response.with_header(h);
            }
            if let Ok(h) = Header::from_bytes(&b"Content-Type"[..], &b"application/octet-stream"[..]) {
                response = response.with_header(h);
            }
            for header in cors_headers() {
                response = response.with_header(header);
            }
            let _sent = request.respond(response);
        }
        Err(reply) => respond_json(request, &reply),
    }
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
        Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, OPTIONS"[..]),
        Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type, Last-Event-ID"[..]),
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
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Last-Event-ID"))
        .and_then(|h| h.value.as_str().parse().ok())
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
        assert_eq!(split_url("/api/stream?agent=a1&ticket=x"), ("/api/stream".to_owned(), "agent=a1&ticket=x".to_owned()));
        assert_eq!(split_url("/api/fleet"), ("/api/fleet".to_owned(), String::new()));
    }
}
