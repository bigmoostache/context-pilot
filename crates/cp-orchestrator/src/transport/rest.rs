//! REST resource handlers — the request/response half of the transport.
//!
//! Every read response is wrapped in an [`Envelope`] carrying the `rev` it
//! reflects, so a client always knows how current its data is and can resume an
//! SSE stream from exactly that point. Actions (`POST /command`) return a
//! [`CommandReceipt`] with the `cmd_id`, the caller's `dedup_token`, and the
//! durable `rev` the effect landed at — everything a client needs to retry
//! safely (the agent's seen-set deduplicates a replay).
//!
//! Handlers return a transport-agnostic [`HttpReply`] (status + JSON body); the
//! server module turns that into a concrete `tiny_http` response. This keeps
//! the handlers pure and unit-testable without a live socket.
//!
//! Concurrency: each handler locks the shared [`Backend`](super::Backend) only
//! briefly to read projected state, and **never** holds the lock across the
//! blocking agent I/O of a body hydrate or command send.

use std::sync::Mutex;

use cp_wire::types::command::Command;
use cp_wire::types::registry::Entry;
use cp_wire::types::ContentHash;
use serde::Serialize;

use super::Backend;
use crate::channel::AgentChannel;
use crate::services::Verdict;

/// A transport-agnostic reply: an HTTP status and a JSON body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpReply {
    /// HTTP status code.
    pub status: u16,
    /// JSON-encoded response body.
    pub body: String,
}

impl HttpReply {
    /// A `200 OK` carrying `value` serialized as JSON.
    fn ok<T: Serialize>(value: &T) -> Self {
        Self::json(200, value)
    }

    /// A reply with `status` carrying `value` serialized as JSON. Serialization
    /// is infallible for our own types; a failure degrades to a `500`.
    fn json<T: Serialize>(status: u16, value: &T) -> Self {
        match serde_json::to_string(value) {
            Ok(body) => Self { status, body },
            Err(_) => Self::error(500, "serialization failed"),
        }
    }

    /// An error reply with a `{"error": reason}` body.
    fn error(status: u16, reason: &str) -> Self {
        Self { status, body: format!("{{\"error\":{}}}", json_string(reason)) }
    }
}

/// A read response wrapping its payload with the `rev` it reflects.
#[derive(Debug, Serialize)]
pub struct Envelope<T: Serialize> {
    /// The oplog `rev` this payload reflects — an SSE stream can resume here.
    pub rev: u64,
    /// The resource payload.
    pub data: T,
}

/// The receipt returned for an accepted (or deduplicated) command.
#[derive(Debug, Serialize)]
pub struct CommandReceipt {
    /// Transport-level command id echoed from the agent's ack.
    pub cmd_id: String,
    /// The caller's semantic dedup token — replay-safe retry key.
    pub dedup_token: String,
    /// Durable `rev` the effect landed at, if accepted.
    pub rev: Option<u64>,
    /// `"accepted"` or `"rejected"`.
    pub status: String,
}

/// `GET /api/fleet` — every known agent's projected view.
///
/// The envelope `rev` is the maximum `rev` across all agents, so a client can
/// open one fleet-wide stream from that point.
pub fn fleet(state: &Mutex<Backend>) -> HttpReply {
    let Ok(backend) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let mut agents = serde_json::Map::new();
    let mut max_rev = 0u64;
    for id in backend.view.agent_ids() {
        if let Some(view) = backend.view.get(id) {
            max_rev = max_rev.max(view.rev);
            if let Ok(value) = serde_json::to_value(view) {
                let _prev = agents.insert(id.to_owned(), value);
            }
        }
    }
    HttpReply::ok(&Envelope { rev: max_rev, data: agents })
}

/// `GET /api/agent/{id}` — one agent's projected view, or `404`.
pub fn agent(state: &Mutex<Backend>, id: &str) -> HttpReply {
    let Ok(backend) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    match backend.view.get(id) {
        Some(view) => HttpReply::ok(&Envelope { rev: view.rev, data: view }),
        None => HttpReply::error(404, "unknown agent"),
    }
}

/// `POST /api/ticket` — mint a single-use SSE upgrade ticket.
pub fn mint_ticket(state: &Mutex<Backend>) -> HttpReply {
    let Ok(mut backend) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let token = backend.tickets.mint();
    HttpReply { status: 200, body: format!("{{\"ticket\":{}}}", json_string(&token)) }
}

/// `GET /api/agent/{id}/body/{hash}` — hydrate a content-addressed body.
///
/// Returns the raw bytes as a JSON string-free octet payload on success, `400`
/// for a malformed hash, `404` for an unknown agent or absent/inlined body, and
/// `502` if the agent's store cannot be read.
pub fn body(state: &Mutex<Backend>, id: &str, hash_hex: &str) -> HttpReply {
    let Some(hash) = ContentHash::from_hex(hash_hex) else {
        return HttpReply::error(400, "malformed content hash");
    };
    let entry = match resolve_entry(state, id) {
        Ok(entry) => entry,
        Err(reply) => return reply,
    };
    // Hydrate is blocking agent I/O — performed with no lock held.
    match AgentChannel::from_entry(&entry).hydrate(hash) {
        Ok(Some(bytes)) => HttpReply {
            status: 200,
            body: serde_json::to_string(&BodyPayload { bytes: &bytes }).unwrap_or_default(),
        },
        Ok(None) => HttpReply::error(404, "body not found"),
        Err(_) => HttpReply::error(502, "body read failed"),
    }
}

/// `POST /api/agent/{id}/command` — submit a command to an agent.
///
/// Fails closed with `503` if the agent's cost breaker is tripped (R2-8/V9),
/// `400` for a malformed command, `404` for an unknown agent, and `502` if the
/// agent is unreachable.
pub fn command(state: &Mutex<Backend>, id: &str, body_bytes: &[u8]) -> HttpReply {
    let Ok(command) = serde_json::from_slice::<Command>(body_bytes) else {
        return HttpReply::error(400, "malformed command");
    };

    // Fail-closed breaker check under a brief lock.
    {
        let Ok(backend) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        if backend.breaker.check(id) == Verdict::Tripped {
            return HttpReply::json(503, &TrippedBody { status: "tripped" });
        }
    }

    let entry = match resolve_entry(state, id) {
        Ok(entry) => entry,
        Err(reply) => return reply,
    };
    let dedup_token = command.dedup_token.clone();
    match AgentChannel::from_entry(&entry).send(command) {
        Ok(ack) => HttpReply::ok(&CommandReceipt {
            cmd_id: ack.cmd_id,
            dedup_token,
            rev: ack.rev,
            status: ack_status(&ack.status),
        }),
        Err(_) => HttpReply::error(502, "agent unreachable"),
    }
}

/// Load an agent's registry [`Entry`] from the configured agents directory.
///
/// Returns an [`HttpReply`] error directly so handlers can `?`-style early-out.
fn resolve_entry(state: &Mutex<Backend>, id: &str) -> Result<Entry, HttpReply> {
    let dir = {
        let backend = state.lock().map_err(|_| HttpReply::error(500, "backend lock poisoned"))?;
        backend.agents_dir.clone()
    };
    let path = dir.join(format!("{id}.json"));
    let raw = std::fs::read(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => HttpReply::error(404, "unknown agent"),
        _ => HttpReply::error(502, "registry read failed"),
    })?;
    serde_json::from_slice::<Entry>(&raw).map_err(|_| HttpReply::error(502, "registry record corrupt"))
}

/// Map an [`Ack`](cp_wire::types::ack::Ack) status to a short string.
fn ack_status(status: &cp_wire::types::ack::Status) -> String {
    match status {
        cp_wire::types::ack::Status::Accepted => "accepted".to_owned(),
        cp_wire::types::ack::Status::Rejected { .. } => "rejected".to_owned(),
    }
}

/// `GET /api/agent/{id}/threads` — thread list with full message logs.
///
/// Reads the agent's `config.json` (via [`StateReader`](crate::inspect::StateReader)),
/// extracts `modules.threads.threads`, and reshapes each thread to the
/// maquette `ThreadDetail` shape (camelCase, `agentId` injected, messages
/// mapped to `log`).
pub fn threads(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let entry = match resolve_entry(state, agent_id) {
        Ok(e) => e,
        Err(reply) => return reply,
    };
    let config = {
        let Ok(mut b) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        b.inspect_mut().read_config(std::path::Path::new(&entry.folder)).ok()
    };
    let Some(config) = config else {
        return HttpReply::error(404, "agent state unavailable");
    };
    let empty_arr = serde_json::Value::Array(Vec::new());
    let raw_threads = config
        .get("modules")
        .and_then(|m| m.get("threads"))
        .and_then(|t| t.get("threads"))
        .and_then(serde_json::Value::as_array)
        .unwrap_or(&empty_arr.as_array().expect("empty vec is array"));

    let details: Vec<serde_json::Value> = raw_threads
        .iter()
        .map(|t| reshape_thread(t, agent_id))
        .collect();
    HttpReply::ok(&details)
}

/// Reshape one raw thread from agent state to the maquette `ThreadDetail`
/// shape: snake_case → camelCase, computed fields (`messageCount`, `unread`,
/// `lastMessage`, `lastActivity`), and messages mapped to `log`.
fn reshape_thread(raw: &serde_json::Value, agent_id: &str) -> serde_json::Value {
    let messages = raw.get("messages").and_then(serde_json::Value::as_array);
    let msg_count = messages.map_or(0, Vec::len);
    let unread = messages.map_or(0, |msgs| {
        msgs.iter().filter(|m| m.get("acknowledged") == Some(&serde_json::Value::Bool(false))).count()
    });
    let last_msg = messages
        .and_then(|msgs| msgs.last())
        .and_then(|m| m.get("content"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let last_activity = messages
        .and_then(|msgs| msgs.last())
        .and_then(|m| m.get("timestamp"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);

    let log: Vec<serde_json::Value> = messages
        .map(|msgs| msgs.iter().enumerate().map(|(i, m)| reshape_message(m, i)).collect())
        .unwrap_or_default();

    let status_str = match raw.get("status").and_then(serde_json::Value::as_str).unwrap_or("TheirTurn") {
        "MyTurn" => "MY_TURN",
        _ => "THEIR_TURN",
    };

    serde_json::json!({
        "id": raw.get("id").and_then(serde_json::Value::as_str).unwrap_or(""),
        "name": raw.get("name").and_then(serde_json::Value::as_str).unwrap_or(""),
        "status": status_str,
        "agentId": agent_id,
        "lastMessage": last_msg,
        "lastActivity": last_activity,
        "messageCount": msg_count,
        "unread": unread,
        "log": log,
    })
}

/// Reshape one thread message to the maquette `ThreadMsg` shape.
fn reshape_message(raw: &serde_json::Value, index: usize) -> serde_json::Value {
    let role = match raw.get("author").and_then(serde_json::Value::as_str).unwrap_or("User") {
        "Assistant" => "assistant",
        _ => "user",
    };
    let mut msg = serde_json::json!({
        "id": format!("msg_{index}"),
        "role": role,
        "content": raw.get("content").and_then(serde_json::Value::as_str).unwrap_or(""),
        "timestamp": raw.get("timestamp").and_then(serde_json::Value::as_u64).unwrap_or(0),
    });
    if let Some(fp) = raw.get("file_path").and_then(serde_json::Value::as_str) {
        let _prev = msg.as_object_mut().expect("just built").insert("fileRef".to_owned(), serde_json::Value::String(fp.to_owned()));
    }
    if let Some(q) = raw.get("question") {
        if !q.is_null() {
            let _prev = msg.as_object_mut().expect("just built").insert("questions".to_owned(), serde_json::json!([q]));
        }
    }
    msg
}

/// JSON-encode a string (with surrounding quotes and escaping).
fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_owned())
}

/// The JSON body for a hydrated body: bytes serialized as a number array.
#[derive(Serialize)]
struct BodyPayload<'a> {
    /// Raw body bytes.
    bytes: &'a [u8],
}

/// The JSON body returned when the cost breaker is tripped.
#[derive(Serialize)]
struct TrippedBody {
    /// Always `"tripped"`.
    status: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{CostBreaker, MaterializedView};
    use cp_wire::types::oplog::{OpEntry, OpEntryKind};
    use cp_wire::types::Phase;
    use std::path::PathBuf;

    fn phase_entry(rev: u64, phase: Phase) -> OpEntry {
        OpEntry { schema_version: 1, rev, timestamp_ms: 0, kind: OpEntryKind::PhaseTransition { phase } }
    }

    fn backend_with_agent() -> Mutex<Backend> {
        let mut view = MaterializedView::new();
        view.apply("a1", &phase_entry(7, Phase::Streaming));
        Mutex::new(Backend::for_test(PathBuf::from("/tmp/cp-test-agents"), view, CostBreaker::new(5.0)))
    }

    #[test]
    fn fleet_lists_agents_with_max_rev() {
        let state = backend_with_agent();
        let reply = fleet(&state);
        assert_eq!(reply.status, 200);
        assert!(reply.body.contains("\"rev\":7"), "envelope carries the max rev");
        assert!(reply.body.contains("a1"), "agent id present");
    }

    #[test]
    fn agent_returns_view_with_rev() {
        let state = backend_with_agent();
        let reply = agent(&state, "a1");
        assert_eq!(reply.status, 200);
        assert!(reply.body.contains("\"rev\":7"));
    }

    #[test]
    fn agent_unknown_is_404() {
        let state = backend_with_agent();
        assert_eq!(agent(&state, "ghost").status, 404);
    }

    #[test]
    fn mint_ticket_returns_token() {
        let state = backend_with_agent();
        let reply = mint_ticket(&state);
        assert_eq!(reply.status, 200);
        assert!(reply.body.contains("\"ticket\""));
    }

    #[test]
    fn body_rejects_malformed_hash() {
        let state = backend_with_agent();
        assert_eq!(body(&state, "a1", "not-a-hash").status, 400);
    }

    #[test]
    fn command_rejects_malformed_body() {
        let state = backend_with_agent();
        assert_eq!(command(&state, "a1", b"{not json").status, 400);
    }

    #[test]
    fn command_fails_closed_when_breaker_tripped() {
        let mut view = MaterializedView::new();
        view.apply("a1", &phase_entry(1, Phase::Idle));
        let mut breaker = CostBreaker::new(5.0);
        breaker.observe("a1", 99.0); // over budget ⇒ tripped
        let state = Mutex::new(Backend::for_test(PathBuf::from("/tmp/x"), view, breaker));

        let cmd = b"{\"schema_version\":1,\"id\":\"c1\",\"seq\":1,\"dedup_token\":\"d1\",\"kind\":{\"kind\":\"stop\"}}";
        let reply = command(&state, "a1", cmd);
        assert_eq!(reply.status, 503, "tripped breaker blocks the command");
        assert!(reply.body.contains("tripped"));
    }
}
