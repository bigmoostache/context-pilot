//! Unit tests for [`Intake`](super::Intake), split from `mod.rs` for the
//! 500-line cap.

use super::*;
use std::io::{Read as _, Write as _};
use tempfile::tempdir;

const TOKEN: &str = "cap-token-256bit";

fn frame(auth: &str, dedup: &str) -> Vec<u8> {
    frame_of(auth, dedup, Kind::SendMessage { agent_id: "T1".to_owned(), content: "hi".to_owned() })
}

fn frame_of(auth: &str, dedup: &str, kind: Kind) -> Vec<u8> {
    let cf = CommandFrame::new(auth.to_owned(), Command::new(format!("cmd-{dedup}"), 1, dedup.to_owned(), kind));
    framing::encode_raw(&serde_json::to_vec(&cf).expect("ser")).expect("frame")
}

#[test]
fn unknown_kind_is_rejected_and_not_journalled() {
    let dir = tempdir().expect("dir");
    let oplog = OplogService::spawn(dir.path()).expect("spawn");
    let mut intake = Intake::new(dir.path(), TOKEN.to_owned()).expect("intake");

    let (ack_bytes, to_apply) = intake.handle_frame(&oplog, &frame_of(TOKEN, "dt-u", Kind::Unknown));
    assert!(matches!(decode_ack(&ack_bytes).status, Status::Rejected { .. }), "unknown kind must be rejected");
    assert!(to_apply.is_none(), "an unknown kind is never applied");

    oplog.shutdown().expect("shutdown");
    let state = replay::replay(dir.path()).expect("replay");
    assert!(!state.seen.contains("dt-u"), "a rejected unknown kind leaves no effect in the log");
}

fn decode_ack(bytes: &[u8]) -> Ack {
    let (payload, _c) = framing::decode_raw(bytes).expect("decode frame");
    serde_json::from_slice(payload).expect("decode ack")
}

#[test]
fn valid_command_is_journalled_and_accepted() {
    let dir = tempdir().expect("dir");
    let oplog = OplogService::spawn(dir.path()).expect("spawn");
    let mut intake = Intake::new(dir.path(), TOKEN.to_owned()).expect("intake");

    let (ack_bytes, to_apply) = intake.handle_frame(&oplog, &frame(TOKEN, "dt-1"));
    let ack = decode_ack(&ack_bytes);
    assert_eq!(ack.status, Status::Accepted);
    assert_eq!(ack.rev, Some(0), "first durable effect lands at rev 0");
    assert!(to_apply.is_some(), "a fresh accept hands back the command to apply");

    oplog.shutdown().expect("shutdown");
    let state = replay::replay(dir.path()).expect("replay");
    assert!(state.seen.contains("dt-1"), "effect is durable in the log");
}

#[test]
fn bad_bearer_is_rejected_and_not_journalled() {
    let dir = tempdir().expect("dir");
    let oplog = OplogService::spawn(dir.path()).expect("spawn");
    let mut intake = Intake::new(dir.path(), TOKEN.to_owned()).expect("intake");

    let (ack_bytes, to_apply) = intake.handle_frame(&oplog, &frame("wrong-token", "dt-x"));
    let ack = decode_ack(&ack_bytes);
    assert!(matches!(ack.status, Status::Rejected { .. }), "bad bearer must be rejected");
    assert!(to_apply.is_none(), "a rejected command is never applied");

    oplog.shutdown().expect("shutdown");
    let state = replay::replay(dir.path()).expect("replay");
    assert!(!state.seen.contains("dt-x"), "a rejected command must not be journalled");
}

#[test]
fn empty_bearer_is_rejected() {
    let dir = tempdir().expect("dir");
    let oplog = OplogService::spawn(dir.path()).expect("spawn");
    let mut intake = Intake::new(dir.path(), TOKEN.to_owned()).expect("intake");

    let (ack_bytes, _none) = intake.handle_frame(&oplog, &frame("", "dt-y"));
    assert!(matches!(decode_ack(&ack_bytes).status, Status::Rejected { .. }));
    oplog.shutdown().expect("shutdown");
}

#[test]
fn malformed_frame_is_rejected() {
    let dir = tempdir().expect("dir");
    let oplog = OplogService::spawn(dir.path()).expect("spawn");
    let mut intake = Intake::new(dir.path(), TOKEN.to_owned()).expect("intake");

    let (ack_bytes, to_apply) = intake.handle_frame(&oplog, b"not a valid frame at all");
    assert!(matches!(decode_ack(&ack_bytes).status, Status::Rejected { .. }));
    assert!(to_apply.is_none());
    oplog.shutdown().expect("shutdown");
}

#[test]
fn duplicate_token_is_idempotent_exactly_once() {
    let dir = tempdir().expect("dir");
    let oplog = OplogService::spawn(dir.path()).expect("spawn");
    let mut intake = Intake::new(dir.path(), TOKEN.to_owned()).expect("intake");

    let (first, apply1) = intake.handle_frame(&oplog, &frame(TOKEN, "dt-dup"));
    assert_eq!(decode_ack(&first).status, Status::Accepted);
    assert!(apply1.is_some(), "first delivery applies");

    let (second, apply2) = intake.handle_frame(&oplog, &frame(TOKEN, "dt-dup"));
    let ack2 = decode_ack(&second);
    assert_eq!(ack2.status, Status::Accepted, "a duplicate is still acknowledged accepted");
    assert_eq!(ack2.rev, Some(0), "the duplicate ack carries the original rev");
    assert!(apply2.is_none(), "a duplicate is never applied a second time");

    oplog.shutdown().expect("shutdown");
    let state = replay::replay(dir.path()).expect("replay");
    assert_eq!(state.seen.len(), 1, "exactly one durable effect for a repeated token");
}

#[test]
fn dedup_survives_deadman_reexec() {
    // V2: an effect journalled before a re-exec must not be re-applied
    // after it. Reopening the oplog + a fresh Intake re-seeds the seen-set
    // from the durable log, so a redelivered command is recognised as done.
    let dir = tempdir().expect("dir");
    {
        let oplog = OplogService::spawn(dir.path()).expect("spawn");
        let mut intake = Intake::new(dir.path(), TOKEN.to_owned()).expect("intake");
        let (_ack, apply) = intake.handle_frame(&oplog, &frame(TOKEN, "dt-survive"));
        assert!(apply.is_some(), "accepted before the crash");
        oplog.shutdown().expect("shutdown");
    } // simulated deadman: process state gone, only the durable log remains.

    let oplog = OplogService::spawn(dir.path()).expect("respawn");
    let mut intake = Intake::new(dir.path(), TOKEN.to_owned()).expect("reopen intake");
    let (ack_bytes, apply) = intake.handle_frame(&oplog, &frame(TOKEN, "dt-survive"));
    assert_eq!(decode_ack(&ack_bytes).status, Status::Accepted);
    assert!(apply.is_none(), "after re-exec the redelivered command is NOT re-applied");

    oplog.shutdown().expect("shutdown");
    let state = replay::replay(dir.path()).expect("replay");
    assert_eq!(state.seen.len(), 1, "still exactly one effect across the re-exec");
}

#[test]
fn handle_connection_acks_a_streamed_command() {
    use std::thread;
    let dir = tempdir().expect("dir");
    let oplog = OplogService::spawn(dir.path()).expect("spawn");
    let mut intake = Intake::new(dir.path(), TOKEN.to_owned()).expect("intake");

    let (mut server, mut client) = UnixStream::pair().expect("socketpair");

    // A commander writes one framed command, then closes its end.
    let writer = thread::spawn(move || {
        client.write_all(&frame(TOKEN, "dt-conn")).expect("write");
        // Read the ack the server writes back.
        let mut chunk = [0u8; 256];
        let mut buf: Vec<u8> = Vec::new();
        loop {
            let n = client.read(&mut chunk).expect("read ack");
            if n == 0 {
                break;
            }
            if let Some(got) = chunk.get(..n) {
                buf.extend_from_slice(got);
            }
            if framing::decode_raw(&buf).is_ok() {
                break;
            }
        }
        drop(client); // EOF so the server loop returns.
        decode_ack(&buf).status
    });

    // A command-only connection never consults the responder.
    let never = |_q: &cp_wire::types::payload::query::Query| -> cp_wire::types::payload::query::Outcome {
        panic!("no query frame on this connection");
    };
    let applied = intake.handle_connection(&oplog, &mut server, &never).expect("handle");
    assert_eq!(applied.len(), 1, "one fresh command was accepted off the connection");
    let status = writer.join().expect("join");
    assert_eq!(status, Status::Accepted, "the commander saw an Accepted ack");

    oplog.shutdown().expect("shutdown");
}
