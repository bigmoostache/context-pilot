//! Phase 30 — the transport's **REST + SSE surface exercised over a real
//! `tiny_http` server on the wire** (design doc §9, roadmap P7-P8).
//!
//! The inline unit tests in `transport/{rest,sse,ticket}.rs` prove the pure
//! pieces (handlers against a `Backend::for_test`, SSE encoding, ticket
//! single-use) without a socket. This suite binds the real server on an
//! ephemeral port and drives it with a hand-rolled blocking HTTP/1.1 + SSE
//! client (see [`common`]), proving the acceptor loop, URL routing, body
//! reading, status codes, the ticket gate, and reconnect-replay-by-`rev` all
//! work end-to-end on the wire.
//!
//! * **REST envelopes.** `GET /api/fleet` and `/api/agent/{id}` carry the
//!   `rev` they reflect; an unknown agent is a real `404`.
//! * **Actions.** `POST /api/ticket` mints a token; `POST .../command` is a
//!   real `400` on malformed input.
//! * **The SSE ticket gate.** `/api/stream` is `401` without a valid ticket and
//!   single-use: a redeemed ticket cannot open a second stream.
//! * **Reconnect-replay by `rev`.** A `Last-Event-ID` header resumes the oplog
//!   tail from that `rev`; only newer deltas are delivered.

mod common;

// Linked into this integration-test target but not named directly; acknowledge
// them for the per-target `unused-crate-dependencies` lint.
use argon2 as _;
use base64 as _;
use calamine as _;
use cp_base as _;
use cp_mod_bridge as _;
use cp_vault as _;
use csv as _;
use dotenvy as _;
use minisign_verify as _;
use nix as _;
use notify as _;
use openssl as _;
use portable_pty as _;
use reqwest as _;
use rusqlite as _;
use serde as _;
use serde_yaml as _;
use sha2 as _;
use utoipa as _;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cp_oplog::append::OplogWriter;

use cp_orchestrator::channel::Tailer;
use cp_orchestrator::transport::{Backend, serve_bound};

use cp_wire::types::ContentHash;
use cp_wire::types::oplog::{OpEntry, OpEntryKind};
use cp_wire::types::registry::{AgentStatus, Entry};

use tiny_http::Server;

use tempfile::{TempDir, tempdir};

/// Build a registry [`Entry`] with the full schema, pointing at `oplog_dir`.
fn make_entry(id: &str, oplog_dir: &Path) -> Entry {
    Entry {
        schema_version: 1,
        id: id.to_owned(),
        folder: "/tmp/agent".to_owned(),
        pid: std::process::id(),
        boot_id: "boot-xyz".to_owned(),
        model: "test-model".to_owned(),
        protocol_version: 1,
        binary_version: "0.0.0".to_owned(),
        socket_path: oplog_dir.join("stream.sock").to_string_lossy().into_owned(),
        oplog_path: oplog_dir.to_string_lossy().into_owned(),
        heartbeat_path: oplog_dir.join("hb").to_string_lossy().into_owned(),
        cap_token: "cap-token".to_owned(),
        started_at_ms: 0,
        status: AgentStatus::Running,
    }
}

/// Write an agent's registry record to `<agents_dir>/<id>.json`.
fn write_entry(agents_dir: &Path, entry: &Entry) {
    let json = serde_json::to_string(entry).expect("serialize entry");
    std::fs::write(agents_dir.join(format!("{}.json", entry.id)), json).expect("write entry");
}

/// A `MessageCreated` head keyed by `byte`.
fn message(byte: u8) -> OpEntryKind {
    OpEntryKind::MessageCreated {
        thread_id: "T1".to_owned(),
        message_id: format!("m{byte}"),
        head: ContentHash::new([byte; 32]),
        inline_body: None,
    }
}

/// Replay an oplog directory into a flat entry list (for seeding the view).
fn replay_entries(oplog_dir: &Path) -> Vec<OpEntry> {
    Tailer::new(oplog_dir.to_path_buf()).poll().expect("poll")
}

/// A running backend on an ephemeral port. Keeps the temp dirs alive for the
/// duration of the test.
struct Harness {
    /// `host:port` the server is bound to.
    addr: String,
    /// Shared backend state.
    _state: Arc<Mutex<Backend>>,
    /// Agents directory (held so it outlives the server).
    _agents: TempDir,
    /// Oplog directory (held so it outlives the server).
    _oplog: TempDir,
}

/// Boot a backend serving one discoverable agent whose oplog holds `n_msgs`
/// message entries, on a freshly-claimed ephemeral port.
fn harness(agent_id: &str, n_msgs: u8) -> Harness {
    let agents = tempdir().expect("agents dir");
    let oplog = tempdir().expect("oplog dir");

    // Seed the agent's oplog with real entries for the stream to deliver.
    let mut writer = OplogWriter::open(oplog.path()).expect("open oplog");
    for byte in 0..n_msgs {
        let _rev = writer.append(message(byte)).expect("append");
    }
    writer.sync().expect("sync");

    let entry = make_entry(agent_id, oplog.path());
    write_entry(agents.path(), &entry);

    let mut backend = Backend::new(
        agents.path().to_path_buf(),
        std::path::PathBuf::from("/tmp/cp-test-realms"),
        std::path::PathBuf::from("/tmp/cp-test-bin"),
        None,
        Duration::from_secs(3600),
    );
    backend.view_mut().apply_batch(agent_id, &replay_entries(oplog.path()));
    let state = Arc::new(Mutex::new(backend));

    let server = Server::http("127.0.0.1:0").expect("bind ephemeral");
    let addr = server.server_addr().to_string();
    let serve_state = Arc::clone(&state);
    let _acceptor = thread::spawn(move || serve_bound(server, serve_state));

    Harness { addr, _state: state, _agents: agents, _oplog: oplog }
}

/// Mint a ticket over the real server and return its token.
fn ticket_token(h: &Harness) -> String {
    let body = common::post_json(&h.addr, "/api/ticket", b"").body;
    body.split_once("\"ticket\":\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(token, _)| token.to_owned())
        .expect("ticket token in body")
}

// ── 1. REST envelopes over the wire ─────────────────────────────────────────

#[test]
fn rest_fleet_and_agent_carry_rev_envelopes() {
    let h = harness("agent-a", 3);

    let fleet = common::get(&h.addr, "/api/fleet", &[]);
    assert_eq!(fleet.status, 200, "fleet served");
    assert!(fleet.body.contains("\"rev\""), "fleet wraps a rev envelope");
    assert!(fleet.body.contains("agent-a"), "the agent appears in the fleet");

    let agent = common::get(&h.addr, "/api/agent/agent-a", &[]);
    assert_eq!(agent.status, 200);
    assert!(agent.body.contains("\"rev\""), "agent view wraps a rev");

    let missing = common::get(&h.addr, "/api/agent/ghost", &[]);
    assert_eq!(missing.status, 404, "unknown agent is a real 404");
}

// ── 1b. the /healthz readiness probe on the wire ─────────────────────────────

/// V2.1a — a bound server with a readable registry (and no auth DB required)
/// answers `200` on `/healthz` from loopback, with a booleans-only body.
#[test]
fn healthz_answers_200_on_the_wire() {
    let h = harness("agent-hz", 1);

    let reply = common::get(&h.addr, "/healthz", &[]);
    assert_eq!(reply.status, 200, "healthy box: {}", reply.body);
    assert!(reply.body.contains("\"status\":\"ok\""), "status ok: {}", reply.body);
    assert!(!reply.body.contains('/'), "no path or secret in the body: {}", reply.body);
}

// ── 2. actions: ticket mint, command 400 ─────────────────────────────────────

#[test]
fn ticket_mint_and_command_status_codes() {
    let h = harness("agent-b", 1);

    let ticket = common::post_json(&h.addr, "/api/ticket", b"");
    assert_eq!(ticket.status, 200);
    assert!(ticket.body.contains("\"ticket\""), "a ticket token is minted");

    let bad = common::post_json(&h.addr, "/api/agent/agent-b/command", b"{not json");
    assert_eq!(bad.status, 400, "a malformed command is a real 400");
}

// ── 3. the SSE ticket gate is required and single-use ───────────────────────

#[test]
fn sse_requires_a_valid_single_use_ticket() {
    let h = harness("agent-c", 4);

    // No ticket → 401 (the negative case resolves fast).
    let (no_ticket, _none) =
        common::sse_collect(&h.addr, "/api/stream?agent=agent-c", &[], 1, Duration::from_millis(600));
    assert_eq!(no_ticket, 401, "the stream demands a ticket");

    // A bogus ticket → 401.
    let (bogus, _e) =
        common::sse_collect(&h.addr, "/api/stream?agent=agent-c&ticket=deadbeef", &[], 1, Duration::from_millis(600));
    assert_eq!(bogus, 401, "an unminted ticket is rejected");

    // Mint a real ticket, open the stream resuming from rev 0 → 200 + the
    // replayed tail (a cold connect head-seeds per T123, so historical deltas
    // come from the reconnect-replay path).
    let token = ticket_token(&h);
    let path = format!("/api/stream?agent=agent-c&ticket={token}");
    let (ok, events) = common::sse_collect(&h.addr, &path, &[("Last-Event-ID", "0")], 1, Duration::from_secs(3));
    assert_eq!(ok, 200, "a valid ticket opens the stream");
    assert!(events.iter().any(|e| e.event == "delta"), "the oplog tail streams as deltas");

    // The same ticket cannot open a second stream (single-use).
    let (reused, _e2) = common::sse_collect(&h.addr, &path, &[], 1, Duration::from_millis(800));
    assert_eq!(reused, 401, "a redeemed ticket is single-use");
}

// ── 4. reconnect-replay by rev (Last-Event-ID) ──────────────────────────────

#[test]
fn sse_resumes_from_last_event_id() {
    let h = harness("agent-d", 6);
    let token = ticket_token(&h);
    let path = format!("/api/stream?agent=agent-d&ticket={token}");

    // Resume from rev 2 → only deltas with a higher rev are delivered.
    let (status, events) = common::sse_collect(&h.addr, &path, &[("Last-Event-ID", "2")], 1, Duration::from_secs(3));
    assert_eq!(status, 200);
    let deltas: Vec<u64> = events.iter().filter(|e| e.event == "delta").filter_map(|e| e.id).collect();
    assert!(!deltas.is_empty(), "replay delivers the tail past rev 2");
    assert!(deltas.iter().all(|&rev| rev > 2), "no delta at or below the resumed rev");
    // Each delta carries the JSON-encoded oplog entry as its data payload.
    assert!(
        events.iter().filter(|e| e.event == "delta").all(|e| e.data.contains("\"rev\"")),
        "a delta's data payload is the serialized oplog entry",
    );
}
