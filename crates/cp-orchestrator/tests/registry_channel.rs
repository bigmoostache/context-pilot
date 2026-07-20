//! Phase 27 — the backend's discovery and per-agent channel, driven against the
//! **real artifacts a live agent produces**, across the backend↔agent seam.
//!
//! The inline tests in `registry.rs`, `liveness.rs`, and `channel.rs` prove
//! each piece in isolation with crafted files. This suite proves the seam: an
//! actual [`cp_mod_bridge::boot::Boot`] writes the registry record, heartbeat,
//! socket, and oplog, and the backend then discovers, tails, hydrates, and
//! commands that agent exactly as it will in production.
//!
//! * **A real booted agent is discovered live, then survives graceful shutdown.**
//!   [`AgentRegistry`] reports `Appeared`+`Live` for a freshly booted agent;
//!   after its `Boot` drops the registry record is intentionally kept (the
//!   agent shows as "Disconnected" in the fleet rather than vanishing).
//! * **`Stale` fires only on a `Live → non-live` transition.** First-sight
//!   staleness rides `Appeared`; a beat going stale later fires exactly one
//!   `Stale`, and a still-stale agent stays quiet.
//! * **The body store → oplog → tailer → hydrate chain reconnects.** A spilled
//!   body the agent wrote is picked up by the backend's [`Tailer`] as a
//!   `MessageCreated` head and resolved by [`AgentChannel::hydrate`], integrity
//!   verified; an inlined body is *not* hydratable (it rides its entry).
//! * **A command round-trips backend → real intake and stays exactly-once.**
//!   [`AgentChannel::send`] reaches a real [`cp_mod_bridge::command::Intake`],
//!   is journalled-then-acked, and a redelivery is deduped — one durable
//!   effect, ever.

// These crate dependencies are linked into this integration-test target but not
// named directly; acknowledge them for the per-target `unused-crate-dependencies`
// lint.
use argon2 as _;
use base64 as _;
use calamine as _;
use cp_base as _;
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
use tiny_http as _;
use utoipa as _;

use std::os::unix::net::UnixListener;
use std::path::Path;
use std::thread;
use std::time::SystemTime;

use cp_mod_bridge::body::Store;
use cp_mod_bridge::boot::Boot;
use cp_mod_bridge::command::Intake;

use cp_oplog::replay::replay;
use cp_oplog::service::Service as OplogService;

use cp_orchestrator::channel::{AgentChannel, Tailer};
use cp_orchestrator::liveness::Liveness;
use cp_orchestrator::registry::{AgentRegistry, Event};

use cp_wire::heartbeat::Heartbeat;
use cp_wire::types::ack::Status;
use cp_wire::types::command::{Command, Kind as CommandKind};
use cp_wire::types::oplog::OpEntryKind;
use cp_wire::types::registry::{AgentStatus, Entry};

use tempfile::tempdir;

/// A boot id of the exact 32-hex-char width a heartbeat record requires.
const BOOT_A: &str = "0123456789abcdef0123456789abcdef";

/// Wall-clock milliseconds since the Unix epoch (clamped at the epoch).
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

// ── crafted-artifact helpers (Test 2) ───────────────────────────────────────

/// Build a registry [`Entry`] for `id` whose `pid` is this live test process
/// (so the pid-liveness signal is satisfied) and whose heartbeat lives at
/// `hb_path`.
fn entry(id: &str, hb_path: &Path, oplog_path: &Path, socket_path: &Path) -> Entry {
    Entry {
        schema_version: 1,
        id: id.to_owned(),
        folder: "/tmp/agent".to_owned(),
        pid: std::process::id(),
        boot_id: BOOT_A.to_owned(),
        model: "test-model".to_owned(),
        protocol_version: 1,
        binary_version: "0.0.0".to_owned(),
        socket_path: socket_path.to_string_lossy().into_owned(),
        oplog_path: oplog_path.to_string_lossy().into_owned(),
        heartbeat_path: hb_path.to_string_lossy().into_owned(),
        cap_token: "tok".to_owned(),
        started_at_ms: 0,
        status: AgentStatus::Running,
    }
}

/// A heartbeat for this process at `timestamp_ms`, bound to [`BOOT_A`].
fn heartbeat(timestamp_ms: u64) -> Heartbeat {
    Heartbeat::new(timestamp_ms, 0, std::process::id(), BOOT_A.to_owned())
}

/// Write `record` as `<id>.json` into `dir`.
fn write_record(dir: &Path, record: &Entry) {
    let path = dir.join(format!("{}.json", record.id));
    std::fs::write(path, serde_json::to_vec(record).expect("serialize record")).expect("write");
}

/// Write `hb` to `path` so the verdict can read a real, decodable beat.
fn write_heartbeat(path: &Path, hb: &Heartbeat) {
    std::fs::write(path, hb.encode().expect("encode heartbeat")).expect("write heartbeat");
}

/// A [`SendMessage`](CommandKind::SendMessage) command keyed by `dedup`.
fn send_command(dedup: &str) -> Command {
    Command::new(
        format!("cmd-{dedup}"),
        1,
        dedup.to_owned(),
        CommandKind::SendMessage { thread_id: "T1".to_owned(), content: "hi".to_owned() },
    )
}

// ── 1. a real booted agent is discovered live, then survives drop ───────────

#[test]
fn a_real_agent_is_discovered_live_then_survives_graceful_shutdown() {
    let folder = tempdir().expect("folder");
    let agents = tempdir().expect("agents dir");

    {
        let boot = Boot::start_in(folder.path(), agents.path(), "test-model").expect("boot");
        let id = boot.id().to_owned();

        let mut reg = AgentRegistry::new(agents.path().to_path_buf());
        let first = reg.scan().expect("scan");
        assert!(
            matches!(first.first(), Some(Event::Appeared(e)) if e.id == id),
            "the booted agent appears in the very first scan",
        );
        assert_eq!(reg.liveness(&id), Some(Liveness::Live), "real pid + fresh beat = live");

        // A quiet scan emits nothing while the agent keeps beating.
        assert!(reg.scan().expect("scan").is_empty(), "no change → no events");

        // Tear the agent down: Drop keeps the registry record (the agent
        // shows as Disconnected rather than vanishing). The test process's
        // PID is still alive and the last heartbeat was recent, so no state
        // change is observed — no event emitted.
        drop(boot);
        let after = reg.scan().expect("scan");
        assert!(after.is_empty(), "no Disappeared — record survives graceful drop");
        assert!(!reg.is_empty(), "the agent remains in the fleet");
    }
}

// ── 2. Stale fires only on a Live→non-live transition ───────────────────────

#[test]
fn stale_fires_once_on_transition_and_first_sight_staleness_rides_appeared() {
    let agents = tempdir().expect("agents dir");
    let hb_a = agents.path().join("hb-a");
    let oplog = agents.path().join("oplog");
    let sock = agents.path().join("a.sock");

    // Agent A boots live (fresh beat).
    write_heartbeat(&hb_a, &heartbeat(now_ms()));
    write_record(agents.path(), &entry("a", &hb_a, &oplog, &sock));

    let mut reg = AgentRegistry::new(agents.path().to_path_buf());
    let first = reg.scan().expect("scan");
    assert!(matches!(first.as_slice(), [Event::Appeared(e)] if e.id == "a"));
    assert_eq!(reg.liveness("a"), Some(Liveness::Live));

    // A's beat goes stale → exactly one Stale event.
    write_heartbeat(&hb_a, &heartbeat(0));
    let second = reg.scan().expect("scan");
    assert_eq!(second, vec![Event::Stale("a".to_owned(), Liveness::StaleHeartbeat)]);

    // Still stale → quiet (Stale does not re-fire).
    assert!(reg.scan().expect("scan").is_empty(), "a persistently-stale agent is quiet");

    // Agent B appears already stale: it rides Appeared, with no Stale event.
    let hb_b = agents.path().join("hb-b");
    write_heartbeat(&hb_b, &heartbeat(0));
    write_record(agents.path(), &entry("b", &hb_b, &oplog, &sock));
    let third = reg.scan().expect("scan");
    assert_eq!(third, vec![Event::Appeared(entry("b", &hb_b, &oplog, &sock))]);
    assert_eq!(reg.liveness("b"), Some(Liveness::StaleHeartbeat), "B is stale but only Appeared");
}

// ── 3. body store → oplog → tailer → hydrate reconnects ─────────────────────

#[test]
fn the_tailer_picks_up_a_spilled_body_the_channel_then_hydrates() {
    let dir = tempdir().expect("dir");
    let oplog_dir = dir.path().join("oplog");
    std::fs::create_dir_all(&oplog_dir).expect("mkdir oplog");

    // The agent writes a large (spilled) body and a small (inlined) one.
    let store = Store::open_with_threshold(&oplog_dir, 4).expect("store");
    let spilled = store.put(b"a large body that spills to its own durable file").expect("put");
    assert!(spilled.is_spilled(), "the large body spilled");
    let inlined = store.put(b"sm").expect("put small");
    assert!(!inlined.is_spilled(), "the small body inlined");

    // The agent journals the spilled body's hash as a message head.
    {
        let oplog = OplogService::spawn(&oplog_dir).expect("spawn");
        let _rev = oplog
            .append_durable(OpEntryKind::MessageCreated {
                thread_id: "T1".to_owned(),
                message_id: "m1".to_owned(),
                head: spilled.hash(),
                inline_body: None,
            })
            .expect("journal head");
        oplog.shutdown().expect("shutdown");
    }

    // The backend tails the oplog and sees the message head.
    let mut tailer = Tailer::new(oplog_dir.clone());
    let entries = tailer.poll().expect("poll");
    let head = entries
        .iter()
        .find_map(|e| match &e.kind {
            OpEntryKind::MessageCreated { head, .. } => Some(*head),
            _ => None,
        })
        .expect("the tailer delivered the MessageCreated");
    assert_eq!(head, spilled.hash(), "the tailed head is the spilled body's hash");

    // The channel hydrates the spilled body (integrity verified), but an
    // inlined body has no file to hydrate — it rode its oplog entry.
    let ch = AgentChannel::from_entry(&entry("a", &dir.path().join("hb"), &oplog_dir, &dir.path().join("a.sock")));
    assert_eq!(
        ch.hydrate(head).expect("hydrate"),
        Some(b"a large body that spills to its own durable file".to_vec()),
        "the spilled body hydrates byte-for-byte",
    );
    assert_eq!(ch.hydrate(inlined.hash()).expect("hydrate"), None, "an inlined body has no file");
}

// ── 4. a command round-trips backend → real intake, exactly-once ────────────

#[test]
fn a_command_round_trips_to_a_real_intake_and_stays_exactly_once() {
    let dir = tempdir().expect("dir");
    let oplog_dir = dir.path().join("oplog");
    std::fs::create_dir_all(&oplog_dir).expect("mkdir oplog");
    let sock = dir.path().join("stream.sock");
    let token = "cap-token-secret".to_owned();

    // The agent side: a real oplog + intake serving two connections.
    let listener = UnixListener::bind(&sock).expect("bind");
    let server = {
        let oplog_dir = oplog_dir.clone();
        let token = token.clone();
        thread::spawn(move || {
            let oplog = OplogService::spawn(&oplog_dir).expect("spawn");
            let mut intake = Intake::new(&oplog_dir, token).expect("intake");
            for _ in 0..2 {
                let (mut conn, _addr) = listener.accept().expect("accept");
                let _applied = intake.handle_connection(&oplog, &mut conn).expect("handle");
            }
            oplog.shutdown().expect("shutdown");
        })
    };

    // The backend side: build a channel from a record advertising this socket.
    let record = Entry { cap_token: token.clone(), ..entry("a", &dir.path().join("hb"), &oplog_dir, &sock) };
    let ch = AgentChannel::from_entry(&record);

    // First send is accepted; the same command redelivered is still acked
    // accepted (idempotent) but journals no second effect.
    let first = ch.send_with_retry(send_command("dt-1"), 10).expect("first send");
    assert_eq!(first.status, Status::Accepted, "the command is accepted");
    let second = ch.send(send_command("dt-1")).expect("second send");
    assert_eq!(second.status, Status::Accepted, "a duplicate is still acknowledged");

    server.join().expect("server joined");

    let recovered = replay(&oplog_dir).expect("replay");
    assert_eq!(recovered.seen.len(), 1, "exactly one durable command effect");
}
