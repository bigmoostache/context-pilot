//! Phase 26 — `cp-mod-bridge` tee + intake + body coverage at the public-API
//! boundary, wired together as they run in a live agent.
//!
//! The inline tests in `tee.rs`, `command.rs`, and `body.rs` prove each
//! component in isolation (the producer never blocks, a bad bearer is rejected,
//! a small body inlines). This suite proves the *cross-component* properties
//! that only emerge when the pieces share a real socket and a real oplog:
//!
//! * **A command round-trips over a real connection and stays exactly-once
//!   across a deadman re-exec.** A framed command flows client → server over a
//!   `UnixStream`, is journalled through a live [`Service`], and acked back;
//!   after the whole stack is torn down and respawned, redelivering the same
//!   command is recognised as already-done — applied once, ever.
//! * **One connection drains many commands and dedups repeats inline.** The
//!   intake loop acks every frame, applies the distinct ones, and silently
//!   dedups a repeat within the same stream.
//! * **The I13 barrier holds across a real journal.** A spilled body is durable
//!   and integrity-checked *before* its referencing entry is journalled; a
//!   crash in the gap leaves a harmless orphan that GC — driven by the
//!   replayed heads, exactly as the agent drives it — reclaims, while a
//!   referenced body is kept.
//! * **The tee streams an ordered multi-frame burst** over a real UDS, framed
//!   with the shared length+CRC codec, delivered in `seq` order.

// The bridge's regular dependencies are linked into this integration-test
// target; name the ones we don't reference directly to satisfy the per-target
// `unused-crate-dependencies` lint.
use cp_base as _;
use cp_render as _;
use log as _;
use nix as _;

use std::collections::HashSet;
use std::io::{Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;
use std::time::{Duration, Instant};

use cp_mod_bridge::body::Store;
use cp_mod_bridge::command::Intake;
use cp_mod_bridge::tee::{Outcome, Tee};
use cp_oplog::replay::replay;
use cp_oplog::service::Service as OplogService;
use cp_wire::framing;
use cp_wire::types::command::{Command, Frame as CommandFrame, Kind as CommandKind};
use cp_wire::types::oplog::OpEntryKind;
use cp_wire::types::stream::{Frame as StreamFrame, Kind as StreamKind};
use cp_wire::types::ack::{Ack, Status};
use cp_wire::types::ContentHash;
use tempfile::tempdir;

const TOKEN: &str = "cap-token-256bit-secret";

// ── frame helpers ───────────────────────────────────────────────────────────

/// A framed [`CommandFrame`] authenticated by `auth`, keyed by `dedup`.
fn command_frame(auth: &str, dedup: &str) -> Vec<u8> {
    let cf = CommandFrame {
        schema_version: 1,
        auth: auth.to_owned(),
        command: Command {
            schema_version: 1,
            id: format!("cmd-{dedup}"),
            seq: 1,
            dedup_token: dedup.to_owned(),
            kind: CommandKind::SendMessage { thread_id: "T1".to_owned(), content: "hi".to_owned() },
        },
    };
    framing::encode_raw(&serde_json::to_vec(&cf).expect("serialise frame")).expect("frame")
}

/// Read exactly `expected` framed [`Ack`]s from `stream`, then return (letting
/// the caller drop the client so the server's [`Intake::handle_connection`]
/// read sees EOF and returns). Reading a fixed count — rather than draining to
/// EOF — avoids depending on a `shutdown(Write)`→peer-EOF chain, mirroring the
/// proven inline `handle_connection` test.
fn read_acks(stream: &mut UnixStream, expected: usize) -> Vec<Status> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 512];
    let mut statuses = Vec::new();
    while statuses.len() < expected {
        let n = stream.read(&mut chunk).expect("read ack");
        if n == 0 {
            break; // server closed early — fewer acks than expected.
        }
        if let Some(got) = chunk.get(..n) {
            buf.extend_from_slice(got);
        }
        while let Ok((payload, consumed)) = framing::decode_raw(&buf) {
            let ack: Ack = serde_json::from_slice(payload).expect("decode ack");
            statuses.push(ack.status);
            let _drained: Vec<u8> = buf.drain(..consumed).collect();
        }
    }
    statuses
}

// ── 1. end-to-end command round-trip + deadman re-exec ──────────────────────

#[test]
fn a_command_round_trips_over_a_connection_and_stays_exactly_once_across_reexec() {
    let dir = tempdir().expect("dir");

    // First life: a command flows over a real connection, is journalled, acked.
    {
        let oplog = OplogService::spawn(dir.path()).expect("spawn");
        let mut intake = Intake::new(dir.path(), TOKEN.to_owned()).expect("intake");
        let (mut server, mut client) = UnixStream::pair().expect("socketpair");

        let writer = thread::spawn(move || {
            client.write_all(&command_frame(TOKEN, "dt-1")).expect("write frame");
            let acks = read_acks(&mut client, 1);
            drop(client); // EOF so the server's read loop returns.
            acks
        });

        let applied = intake.handle_connection(&oplog, &mut server).expect("handle");
        assert_eq!(applied.len(), 1, "the command was freshly accepted");
        let statuses = writer.join().expect("writer joined");
        assert_eq!(statuses, vec![Status::Accepted], "the commander saw one Accepted ack");
        oplog.shutdown().expect("shutdown");
    } // deadman: process state gone, only the durable log survives.

    // Second life: respawn and redeliver the same command — not re-applied.
    {
        let oplog = OplogService::spawn(dir.path()).expect("respawn");
        let mut intake = Intake::new(dir.path(), TOKEN.to_owned()).expect("reopen intake");
        let (mut server, mut client) = UnixStream::pair().expect("socketpair");

        let writer = thread::spawn(move || {
            client.write_all(&command_frame(TOKEN, "dt-1")).expect("rewrite frame");
            let acks = read_acks(&mut client, 1);
            drop(client); // EOF so the server's read loop returns.
            acks
        });

        let applied = intake.handle_connection(&oplog, &mut server).expect("handle");
        assert!(applied.is_empty(), "after re-exec the redelivered command is NOT re-applied");
        let statuses = writer.join().expect("writer joined");
        assert_eq!(statuses, vec![Status::Accepted], "a duplicate is still acknowledged accepted");
        oplog.shutdown().expect("shutdown");
    }

    let recovered = replay(dir.path()).expect("replay");
    assert_eq!(recovered.seen.len(), 1, "exactly one durable effect across the re-exec");
}

// ── 2. one connection drains many commands, dedups repeats ──────────────────

#[test]
fn one_connection_applies_distinct_commands_and_dedups_repeats() {
    let dir = tempdir().expect("dir");
    let oplog = OplogService::spawn(dir.path()).expect("spawn");
    let mut intake = Intake::new(dir.path(), TOKEN.to_owned()).expect("intake");
    let (mut server, mut client) = UnixStream::pair().expect("socketpair");

    // Two distinct commands plus a repeat of the first, all on one stream.
    let writer = thread::spawn(move || {
        for dedup in ["dt-a", "dt-b", "dt-a"] {
            client.write_all(&command_frame(TOKEN, dedup)).expect("write frame");
        }
        let acks = read_acks(&mut client, 3);
        drop(client); // EOF so the server's read loop returns.
        acks
    });

    let applied = intake.handle_connection(&oplog, &mut server).expect("handle");
    assert_eq!(applied.len(), 2, "only the two distinct commands are applied");
    let statuses = writer.join().expect("writer joined");
    assert_eq!(statuses.len(), 3, "every frame is acknowledged");
    assert!(statuses.iter().all(|s| *s == Status::Accepted), "all three acks are Accepted");

    oplog.shutdown().expect("shutdown");
    let recovered = replay(dir.path()).expect("replay");
    assert_eq!(recovered.seen.len(), 2, "the repeat journalled no second effect");
}

// ── 3. I13 barrier across a real journal + replay-driven GC ─────────────────

#[test]
fn the_i13_barrier_and_orphan_gc_hold_across_a_real_journal() {
    let dir = tempdir().expect("dir");
    // Tiny threshold so a short body already spills to its own durable file.
    let store = Store::open_with_threshold(dir.path(), 4).expect("store");

    // Crash-in-the-gap: a body spills durably, but its referencing entry is
    // never journalled. The body is durable and integrity-checked now…
    let orphan = store.put(b"a body that spilled then the agent died").expect("put orphan");
    assert!(orphan.is_spilled(), "the body spilled to its own file");
    assert!(store.get(orphan.hash()).expect("get").is_some(), "the spilled body is durable");

    // …and replay sees no reference to it (no MessageCreated was appended).
    let recovered = replay(dir.path()).expect("replay empty");
    assert!(recovered.heads.threads.is_empty(), "no entry references the orphan");

    // The happy path: a second body, journalled into a real MessageCreated.
    let referenced = store.put(b"a body that is properly referenced").expect("put live");
    {
        let oplog = OplogService::spawn(dir.path()).expect("spawn");
        let _rev = oplog
            .append_durable(OpEntryKind::MessageCreated {
                thread_id: "T1".to_owned(),
                message_id: "m1".to_owned(),
                head: referenced.hash(),
            })
            .expect("journal the reference");
        oplog.shutdown().expect("shutdown");
    }

    // GC driven by the replayed heads — exactly how the agent reclaims bodies.
    let recovered = replay(dir.path()).expect("replay with reference");
    let live: HashSet<ContentHash> =
        recovered.heads.threads.iter().map(|h| h.last_message_hash).collect();
    assert!(live.contains(&referenced.hash()), "the journalled head is in the live set");

    let removed = store.gc(&live, Duration::ZERO).expect("gc");
    assert_eq!(removed, 1, "the unreferenced crash-orphan is reclaimed");
    assert_eq!(store.get(orphan.hash()).expect("get"), None, "the orphan file is gone");
    assert!(store.get(referenced.hash()).expect("get").is_some(), "the referenced body is kept");
}

// ── 4. tee streams an ordered multi-frame burst ─────────────────────────────

#[test]
fn the_tee_streams_an_ordered_multi_frame_burst() {
    let socket_dir = tempdir().expect("dir");
    let path = socket_dir.path().join("stream.sock");
    let listener = UnixListener::bind(&path).expect("bind");
    let tee = Tee::spawn(listener);

    let mut observer = UnixStream::connect(&path).expect("connect");
    observer.set_nonblocking(true).expect("nonblocking");
    thread::sleep(Duration::from_millis(80)); // let the publisher accept it.

    let burst = 16u64;
    for seq in 0..burst {
        let frame = StreamFrame {
            schema_version: 1,
            agent_id: "a".to_owned(),
            worker_id: "w".to_owned(),
            thread_id: "T1".to_owned(),
            message_id: "m1".to_owned(),
            seq,
            kind: StreamKind::Token { text: format!("tok{seq}") },
        };
        assert_eq!(tee.publish(frame), Outcome::Published, "within capacity nothing drops");
    }

    // Read every frame back, decoding the shared length+CRC framing, in order.
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 1024];
    let mut seqs: Vec<u64> = Vec::new();
    while seqs.len() < usize::try_from(burst).unwrap_or(0) && Instant::now() < deadline {
        match observer.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if let Some(got) = chunk.get(..n) {
                    buf.extend_from_slice(got);
                }
                while let Ok((payload, consumed)) = framing::decode_raw(&buf) {
                    let frame: StreamFrame = serde_json::from_slice(payload).expect("decode frame");
                    seqs.push(frame.seq);
                    let _drained: Vec<u8> = buf.drain(..consumed).collect();
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(_other) => break,
        }
    }

    let expected: Vec<u64> = (0..burst).collect();
    assert_eq!(seqs, expected, "all frames arrive exactly once, in publish order");

    tee.shutdown();
}
