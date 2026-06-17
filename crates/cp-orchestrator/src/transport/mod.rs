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

pub mod rest;
pub mod sse;
pub mod ticket;

use std::io::{Read as _, Write as _};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tiny_http::{Header, Method, Request, Response, Server};

use cp_wire::types::registry::Entry;
use cp_wire::types::stream::Frame;

use crate::channel::Tailer;
use crate::inspect::StateReader;
use crate::services::{CostBreaker, MaterializedView, StreamHub};
use ticket::TicketStore;

/// Poll interval for the SSE producer between oplog/stream sweeps.
const STREAM_POLL: Duration = Duration::from_millis(200);

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
}

impl Backend {
    /// Build a backend with empty services and the given per-agent cost budget.
    #[must_use]
    pub fn new(agents_dir: PathBuf, budget_usd: f64) -> Self {
        Self {
            view: MaterializedView::new(),
            breaker: CostBreaker::new(budget_usd),
            hub: StreamHub::new(DEFAULT_SUB_CAPACITY),
            tickets: TicketStore::new(),
            inspect: StateReader::new(),
            agents_dir,
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

    /// Construct a backend from explicit services — used by tests.
    #[cfg(test)]
    pub(crate) fn for_test(agents_dir: PathBuf, view: MaterializedView, breaker: CostBreaker) -> Self {
        Self { view, breaker, hub: StreamHub::new(DEFAULT_SUB_CAPACITY), tickets: TicketStore::new(), inspect: StateReader::new(), agents_dir }
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

    // Read the body up-front (only POST routes consume it). The mutable borrow
    // ends here, before the request is moved into the response.
    let body_bytes = if method == Method::Post { read_body(&mut request) } else { Vec::new() };

    let reply = route_rest(&method, &segments, state, body_bytes.as_slice());
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
) -> rest::HttpReply {
    match (method, segments) {
        (Method::Get, ["api", "health"]) => rest::HttpReply { status: 200, body: "{\"status\":\"ok\"}".to_owned() },
        (Method::Get, ["api", "fleet"]) => rest::fleet(state),
        (Method::Get, ["api", "agent", id]) => rest::agent(state, id),
        (Method::Get, ["api", "agent", id, "body", hash]) => rest::body(state, id, hash),
        (Method::Get, ["api", "agent", id, "threads"]) => rest::threads(state, id),
        (Method::Post, ["api", "agent", id, "command"]) => rest::command(state, id, body_bytes),
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
    let _producer = thread::spawn(move || run_stream(&sink, &producer_state, &agent, &oplog_dir, last_rev));

    stream_to_client(request, body);
}

/// Stream an SSE body to the client, flushing **after every event**.
///
/// tiny_http's `Response`/`respond` path copies the whole body through a 1 KiB
/// `BufWriter` and only flushes when that buffer fills or the response *ends* —
/// fatal for an unbounded event stream, where small events would sit unsent in
/// the buffer forever. So we take the raw connection writer, emit the status
/// line and SSE headers ourselves, then copy each chunk the producer yields and
/// **flush immediately**, so every event reaches the browser the instant it is
/// produced. The loop ends when the producer finishes (EOF) or the client
/// disconnects (a write error), at which point dropping `body` signals the
/// producer thread to stop.
fn stream_to_client(request: Request, mut body: sse::SseBody) {
    let mut writer = request.into_writer();
    let preamble = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: text/event-stream\r\n",
        "Cache-Control: no-cache\r\n",
        "Connection: keep-alive\r\n",
        "Access-Control-Allow-Origin: *\r\n",
        "Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n",
        "Access-Control-Allow-Headers: Content-Type, Last-Event-ID\r\n",
        "\r\n",
    );
    if writer.write_all(preamble.as_bytes()).and_then(|()| writer.flush()).is_err() {
        return;
    }

    let mut buf = [0u8; 4096];
    loop {
        match body.read(&mut buf) {
            Ok(0) => break, // producer finished — clean end of stream.
            Ok(n) => {
                let Some(chunk) = buf.get(..n) else { break };
                if writer.write_all(chunk).and_then(|()| writer.flush()).is_err() {
                    break; // client disconnected.
                }
            }
            Err(_) => break,
        }
    }
}

/// The SSE producer loop: replay-from-`rev`, then live oplog + stream tail.
///
/// Runs until a `send` fails (the client disconnected, dropping the body
/// reader). Unsubscribes its stream-hub slot on exit.
fn run_stream(sink: &sse::SseSink, state: &Arc<Mutex<Backend>>, agent_id: &str, oplog_dir: &PathBuf, last_rev: Option<u64>) {
    let mut tailer = Tailer::new(oplog_dir.clone());
    if let Some(rev) = last_rev {
        tailer.seed(rev);
    }
    let sub_id = state.lock().ok().map(|mut b| b.hub.subscribe(agent_id));
    let mut gap_checked = last_rev.is_none();

    loop {
        // Oplog deltas (durable, rev-numbered).
        match tailer.poll() {
            Ok(entries) => {
                if !gap_checked {
                    if let (Some(want), Some(first)) = (last_rev, entries.first()) {
                        // The oldest replayable entry skips past the client's
                        // last rev ⇒ a gap the oplog can't cover ⇒ resync.
                        if first.rev > want.saturating_add(1) && sink.send(&sse::SseMessage::resync()).is_err() {
                            break;
                        }
                    }
                    gap_checked = true;
                }
                for entry in &entries {
                    let data = serde_json::to_string(entry).unwrap_or_default();
                    if sink.send(&sse::SseMessage::delta(entry.rev, data)).is_err() {
                        return cleanup(state, agent_id, sub_id);
                    }
                }
            }
            Err(_) => {
                if sink.send(&sse::SseMessage::resync()).is_err() {
                    return cleanup(state, agent_id, sub_id);
                }
            }
        }

        // Ephemeral stream frames (best-effort hints).
        if let Some(sub) = sub_id {
            let frames = drain_frames(state, agent_id, sub);
            for frame in &frames {
                let data = serde_json::to_string(frame).unwrap_or_default();
                if sink.send(&sse::SseMessage::stream(data)).is_err() {
                    return cleanup(state, agent_id, sub_id);
                }
            }
        }

        // Keep-alive doubles as a disconnect probe.
        if sink.keep_alive().is_err() {
            return cleanup(state, agent_id, sub_id);
        }
        thread::sleep(STREAM_POLL);
    }
    cleanup(state, agent_id, sub_id);
}

/// Drain an agent's stream-hub subscriber buffer under a brief lock.
fn drain_frames(state: &Arc<Mutex<Backend>>, agent_id: &str, sub: u64) -> Vec<Frame> {
    state.lock().ok().and_then(|mut b| b.hub.drain(agent_id, sub)).unwrap_or_default()
}

/// Release the stream-hub subscriber on producer exit.
fn cleanup(state: &Arc<Mutex<Backend>>, agent_id: &str, sub_id: Option<u64>) {
    if let (Ok(mut backend), Some(sub)) = (state.lock(), sub_id) {
        let _removed = backend.hub.unsubscribe(agent_id, sub);
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

/// A parsed query string (`k=v&k2=v2`).
struct QueryParams {
    /// Decoded key/value pairs.
    pairs: Vec<(String, String)>,
}

impl QueryParams {
    /// Parse a raw query string. Values are taken verbatim (no percent-decode;
    /// ticket tokens and agent ids are hex/identifier-safe).
    fn parse(query: &str) -> Self {
        let pairs = query
            .split('&')
            .filter(|s| !s.is_empty())
            .map(|pair| match pair.split_once('=') {
                Some((k, v)) => (k.to_owned(), v.to_owned()),
                None => (pair.to_owned(), String::new()),
            })
            .collect();
        Self { pairs }
    }

    /// Look up the first value for `key`.
    fn get(&self, key: &str) -> Option<&str> {
        self.pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
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

    #[test]
    fn query_params_parse_and_lookup() {
        let q = QueryParams::parse("agent=a1&ticket=deadbeef&last_rev=5");
        assert_eq!(q.get("agent"), Some("a1"));
        assert_eq!(q.get("ticket"), Some("deadbeef"));
        assert_eq!(q.get("last_rev"), Some("5"));
        assert_eq!(q.get("missing"), None);
    }

    #[test]
    fn query_params_handle_empty() {
        let q = QueryParams::parse("");
        assert_eq!(q.get("agent"), None);
    }
}
