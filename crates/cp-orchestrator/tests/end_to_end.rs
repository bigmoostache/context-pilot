//! Phase 30 — the **end-to-end loopback**: a real agent-side stack wired to the
//! real backend transport, driven over the wire by a hand-rolled HTTP client
//! (design doc §9, the capstone of roadmap P7-P8).
//!
//! Everything below is a genuine component talking to another genuine component
//! over a real socket — no mocks, no in-process shortcuts:
//!
//! ```text
//!   HTTP client ──REST POST /command──▶ backend transport
//!                                          │ AgentChannel::send (UDS)
//!                                          ▼
//!                                       agent: Intake.handle_connection
//!                                          │ journal-then-ack (fsync)
//!                                          ▼
//!                                       agent oplog (durable)
//!                                          ▲ Tailer::poll
//!                                          │
//!   HTTP client ◀──SSE delta────────── backend transport
//! ```
//!
//! * **A command journals on the agent and re-emerges as an SSE delta.** A
//!   `POST /api/agent/{id}/command` reaches a real [`Intake`], is durably
//!   journalled, and the receipt carries `cmd_id` + `dedup_token` + the durable
//!   `rev`. The backend's [`Tailer`] then tails that same oplog and the client
//!   streaming `/api/stream` sees the command's effect arrive as a delta —
//!   the whole loop closed across real sockets and real `fsync`s.
//! * **A spilled body hydrates over REST.** A body the agent spilled to its
//!   store is fetched byte-for-byte via `GET /…/body/{hash}`; a malformed hash
//!   is `400` and an absent one `404`.
//! * **The stream survives a soak of connect/disconnect cycles.** Opening and
//!   dropping many SSE streams in a row leaves the server healthy — no hang, no
//!   descriptor exhaustion, every cycle still served.

mod common;

// Linked into this integration-test target but not named directly; acknowledge
// them for the per-target `unused-crate-dependencies` lint.
use nix as _;
use serde as _;
use serde_yaml as _;

use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cp_mod_bridge::body::Store;
use cp_mod_bridge::command::Intake;

use cp_oplog::append::OplogWriter;
use cp_oplog::replay::replay;
use cp_oplog::service::Service as OplogService;

use cp_orchestrator::transport::{serve_bound, Backend};

use cp_wire::types::oplog::OpEntryKind;
use cp_wire::types::registry::{AgentStatus, Entry};
use cp_wire::types::ContentHash;

use tempfile::tempdir;

use tiny_http::Server;

/// Build a registry [`Entry`] advertising the given agent dir's socket and
/// oplog, with this process's pid (so backend pid-liveness is satisfied) and
/// `cap_token`.
fn make_entry(id: &str, dir: &Path, cap_token: &str) -> Entry {
    Entry {
        schema_version: 1,
        id: id.to_owned(),
        folder: "/tmp/agent".to_owned(),
        pid: std::process::id(),
        boot_id: "boot-e2e".to_owned(),
        model: "test-model".to_owned(),
        protocol_version: 1,
        binary_version: "0.0.0".to_owned(),
        socket_path: dir.join("stream.sock").to_string_lossy().into_owned(),
        oplog_path: dir.join("oplog").to_string_lossy().into_owned(),
        heartbeat_path: dir.join("hb").to_string_lossy().into_owned(),
        cap_token: cap_token.to_owned(),
        started_at_ms: 0,
        status: AgentStatus::Running,
    }
}

/// Write an agent's registry record to `<agents_dir>/<id>.json`.
fn write_entry(agents_dir: &Path, entry: &Entry) {
    let json = serde_json::to_string(entry).expect("serialize entry");
    std::fs::write(agents_dir.join(format!("{}.json", entry.id)), json).expect("write entry");
}

/// Bind a backend serving `agents_dir` on an ephemeral port; returns its
/// address. The acceptor thread runs until the process exits.
fn serve_backend(agents_dir: &Path) -> (String, Arc<Mutex<Backend>>) {
    let state = Arc::new(Mutex::new(Backend::new(agents_dir.to_path_buf(), 5.0)));
    let server = Server::http("127.0.0.1:0").expect("bind ephemeral");
    let addr = server.server_addr().to_string();
    let serve_state = Arc::clone(&state);
    let _acceptor = thread::spawn(move || serve_bound(server, serve_state));
    (addr, state)
}

/// Mint a ticket over the real server and return its token.
fn ticket_token(addr: &str) -> String {
    let body = common::post_json(addr, "/api/ticket", b"").body;
    body.split_once("\"ticket\":\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(token, _)| token.to_owned())
        .expect("ticket token in body")
}

/// A `MessageCreated` oplog entry keyed by `byte` (a stand-in stream payload).
fn message_entry(byte: u8) -> OpEntryKind {
    OpEntryKind::MessageCreated {
        thread_id: "T1".to_owned(),
        message_id: format!("m{byte}"),
        head: ContentHash::new([byte; 32]),
    }
}

// ── 1. the capstone loop: POST command → journal → tail → SSE delta ─────────

#[test]
fn a_command_journals_on_the_agent_and_re_emerges_as_an_sse_delta() {
    let agents = tempdir().expect("agents dir");
    let agent_dir = tempdir().expect("agent dir");
    let oplog_dir = agent_dir.path().join("oplog");
    std::fs::create_dir_all(&oplog_dir).expect("mkdir oplog");
    let sock = agent_dir.path().join("stream.sock");
    let cap_token = "cap-token-e2e";

    // ── Agent side: a real oplog + intake serving one command connection. ──
    let listener = UnixListener::bind(&sock).expect("bind socket");
    let agent = {
        let oplog_dir = oplog_dir.clone();
        let cap_token = cap_token.to_owned();
        thread::spawn(move || {
            let oplog = OplogService::spawn(&oplog_dir).expect("spawn oplog");
            let mut intake = Intake::new(&oplog_dir, cap_token).expect("intake");
            let (mut conn, _addr) = listener.accept().expect("accept");
            let _applied = intake.handle_connection(&oplog, &mut conn).expect("handle");
            oplog.shutdown().expect("shutdown");
        })
    };

    // ── Backend side: discover the agent, serve the transport. ──
    let entry = make_entry("agent-e2e", agent_dir.path(), cap_token);
    write_entry(agents.path(), &entry);
    let (addr, _state) = serve_backend(agents.path());

    // ── Client: POST a command; the receipt carries the durable rev. ──
    let cmd = br#"{"schema_version":1,"id":"cmd-e2e","seq":1,"dedup_token":"d-e2e","kind":{"kind":"send_message","thread_id":"T1","content":"hi"}}"#;
    let receipt = common::post_json(&addr, "/api/agent/agent-e2e/command", cmd);
    assert_eq!(receipt.status, 200, "the command is accepted end-to-end");
    assert!(receipt.body.contains("\"cmd_id\":\"cmd-e2e\""), "receipt echoes the cmd id");
    assert!(receipt.body.contains("\"dedup_token\":\"d-e2e\""), "receipt carries the dedup token");
    assert!(receipt.body.contains("\"status\":\"accepted\""), "the receipt says accepted");
    assert!(receipt.body.contains("\"rev\":0"), "the durable effect landed at rev 0");

    agent.join().expect("agent thread joined");

    // The effect is durably on the agent's oplog.
    let recovered = replay(&oplog_dir).expect("replay");
    assert!(recovered.seen.contains("d-e2e"), "the command effect is durable in the log");

    // ── Client: stream the agent; the journalled command arrives as a delta. ──
    let token = ticket_token(&addr);
    let path = format!("/api/stream?agent=agent-e2e&ticket={token}");
    let (status, events) = common::sse_collect(&addr, &path, &[], 1, Duration::from_secs(3));
    assert_eq!(status, 200, "the stream opens");
    let delta = events
        .iter()
        .find(|e| e.event == "delta")
        .expect("the command effect streams back as a delta");
    assert_eq!(delta.id, Some(0), "the delta is tagged with the rev it reflects (id = rev framing)");
    assert!(
        delta.data.contains("d-e2e"),
        "the streamed delta carries the command's dedup token (full loop closed)",
    );
}

// ── 2. a spilled body hydrates over REST ────────────────────────────────────

#[test]
fn a_spilled_body_hydrates_over_rest() {
    let agents = tempdir().expect("agents dir");
    let agent_dir = tempdir().expect("agent dir");
    let oplog_dir = agent_dir.path().join("oplog");
    std::fs::create_dir_all(&oplog_dir).expect("mkdir oplog");

    // The agent spills a large body to its content-addressed store.
    let store = Store::open_with_threshold(&oplog_dir, 4).expect("store");
    let payload = b"a large body that spills to its own durable file on disk";
    let spilled = store.put(payload).expect("put");
    assert!(spilled.is_spilled(), "the body spilled");
    let hash_hex = spilled.hash().to_hex();

    // The backend serves a discoverable agent pointing at that store.
    let entry = make_entry("agent-body", agent_dir.path(), "cap");
    write_entry(agents.path(), &entry);
    let (addr, _state) = serve_backend(agents.path());

    // A good hash hydrates the exact bytes (returned as a JSON number array).
    let ok = common::get(&addr, &format!("/api/agent/agent-body/body/{hash_hex}"), &[]);
    assert_eq!(ok.status, 200, "a known body hydrates");
    let first = i64::from(payload.first().copied().unwrap_or(0));
    assert!(ok.body.contains(&first.to_string()), "the body payload bytes are returned");

    // A malformed hash is a 400; an absent (well-formed) hash is a 404.
    let bad = common::get(&addr, "/api/agent/agent-body/body/not-a-hash", &[]);
    assert_eq!(bad.status, 400, "a malformed hash is rejected");
    let absent_hash = ContentHash::new([0xAB; 32]).to_hex();
    let absent = common::get(&addr, &format!("/api/agent/agent-body/body/{absent_hash}"), &[]);
    assert_eq!(absent.status, 404, "an unknown body is a 404");
}

// ── 3. the stream survives a soak of connect/disconnect cycles ──────────────

#[test]
fn the_stream_survives_a_soak_of_connect_disconnect_cycles() {
    let agents = tempdir().expect("agents dir");
    let agent_dir = tempdir().expect("agent dir");
    let oplog_dir = agent_dir.path().join("oplog");
    std::fs::create_dir_all(&oplog_dir).expect("mkdir oplog");

    // Seed a few entries so every stream has a delta to deliver.
    let mut writer = OplogWriter::open(&oplog_dir).expect("open oplog");
    for byte in 0..3u8 {
        let _rev = writer.append(message_entry(byte)).expect("append");
    }
    writer.sync().expect("sync");

    let entry = make_entry("agent-soak", agent_dir.path(), "cap");
    write_entry(agents.path(), &entry);
    let (addr, _state) = serve_backend(agents.path());

    // Many short-lived stream connections: each mints a ticket, opens the
    // stream, reads at least one delta, then drops. A leaked descriptor or a
    // wedged producer would surface as a hang or a non-200 within the budget.
    for _cycle in 0..25 {
        let token = ticket_token(&addr);
        let path = format!("/api/stream?agent=agent-soak&ticket={token}");
        let (status, events) = common::sse_collect(&addr, &path, &[], 1, Duration::from_secs(2));
        assert_eq!(status, 200, "every soak cycle opens cleanly");
        assert!(events.iter().any(|e| e.event == "delta"), "every cycle delivers a delta");
    }

    // The server is still healthy after the soak: a plain REST call answers.
    let fleet = common::get(&addr, "/api/fleet", &[]);
    assert_eq!(fleet.status, 200, "the server stays healthy after the soak");
}
