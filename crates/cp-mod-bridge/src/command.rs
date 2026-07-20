//! [`Intake`] — the agent-side command intake: authenticate, journal-then-ack,
//! and dedup a backend command into exactly-once durable truth (design doc
//! I9/I11/I4, roadmap P4, MILESTONE M2).
//!
//! # The contract
//!
//! A command arrives over the UDS as a framed [`CommandFrame`] (the bearer
//! credential in an outer envelope, the [`Command`] inside). Intake turns it
//! into a durable, exactly-once effect in three steps:
//!
//! 1. **Authenticate** (I9). The frame's `auth` must equal the agent's
//!    `cap_token` — the 256-bit bearer secret minted at boot and written only
//!    to the `0600` registry file. An empty or mismatched bearer is rejected
//!    *before* the command's effect is ever considered. v1 authn is filesystem
//!    perms + this presence-checked bearer; HMAC / nonce / a timing-safe
//!    compare belong at the remote-transport seam (G7), where the attacker is
//!    genuinely weaker than the local perms boundary.
//! 2. **Journal-then-ack** (I11). The command's effect is appended to the oplog
//!    and **`fdatasync`'d before** an `Accepted` ack is returned (the durable
//!    append runs through [`OplogService::append_durable`], which only returns
//!    once the group commit has synced). So "accepted" always means "durable":
//!    a crash *after* the ack replays the effect; a crash *before* it leaves no
//!    ack, and the commander retries.
//! 3. **Dedup** (I4). Every command carries a semantic `dedup_token`. Intake
//!    holds a [`SeenSet`] seeded from the durable log on construction and
//!    advanced as it accepts; a token already present is acknowledged
//!    **without re-journaling and without re-applying** — idempotent. This is
//!    what makes a deadman re-exec (which replays the log and re-seeds the
//!    seen-set) apply each command *exactly once*: a redelivered command is
//!    recognised as already-done.
//!
//! # What Intake does *not* do
//!
//! It does not *apply* the effect. A fresh acceptance returns the [`Command`]
//! to the caller, which injects it through the agent's normal user-message
//! entry (design doc K7 — `actions/input.rs`, never the spine's
//! auto-continuation). Applying off the **oplog** (not off the socket) is what
//! keeps it idempotent across a re-exec: replay re-drives un-applied effects,
//! and the seen-set guarantees they are not double-applied.
//!
//! # Single writer
//!
//! The oplog has exactly one writer. Intake therefore does **not** own an
//! [`OplogService`]; it is handed one (the agent's single
//! [`crate::boot::Boot`]-owned service) to journal through, so command effects
//! and the stream loop's own records share one durable log.

use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;

use cp_oplog::replay;
use cp_oplog::service::Service as OplogService;
use cp_wire::framing;
use cp_wire::types::ack::{Ack, Status};
use cp_wire::types::command::{Command, Frame as CommandFrame};
use cp_wire::types::oplog::OpEntryKind;
use cp_wire::types::snapshot::SeenSet;

use crate::error::{BootResult, Error};

/// Largest accumulated read (per connection) before a frame is abandoned as
/// junk — bounds memory against a peer that sends an endless un-decodable
/// stream. 32 MiB comfortably fits any realistic command frame (a `SendMessage`
/// carries the full message text); the old 1 MiB cap rejected a large paste,
/// contributing to the "big messages don't go through" symptom (T274). Kept in
/// lockstep with the backend transport's `MAX_BODY` (the other cap on the same
/// path).
const MAX_CONNECTION_BUFFER: usize = 32 * 1024 * 1024;

/// Read-chunk size for [`Intake::handle_connection`].
const READ_CHUNK: usize = 4096;

/// The agent-side command intake authority.
///
/// Holds the bearer to match and the dedup [`SeenSet`] (seeded from the durable
/// log, advanced on each acceptance). One intake serves the agent's single
/// command channel; it journals through a shared [`OplogService`].
#[derive(Debug)]
pub struct Intake {
    /// The bearer a commander must present (the agent's `cap_token`).
    cap_token: String,

    /// Dedup authority: tokens of effects already durable. Seeded from replay
    /// on [`new`](Self::new), advanced as commands are accepted.
    seen: SeenSet,
}

impl Intake {
    /// Build an intake for `cap_token`, seeding its dedup set from the durable
    /// log in `oplog_dir` so a freshly-restarted agent recognises commands it
    /// already accepted before the restart (exactly-once across re-exec).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if the oplog directory cannot be replayed.
    pub fn new(oplog_dir: &std::path::Path, cap_token: String) -> BootResult<Self> {
        let recovered =
            replay::replay(oplog_dir).map_err(|e| Error::io("replay oplog for command dedup", into_io(&e)))?;
        Ok(Self { cap_token, seen: recovered.seen })
    }

    /// Process one framed [`CommandFrame`], returning the framed [`Ack`] to send
    /// back and — only on a **fresh** acceptance — the [`Command`] the caller
    /// must apply.
    ///
    /// A malformed frame, a bad bearer, or a journaling failure all yield a
    /// framed `Ack::Rejected` and no command to apply (fail-closed). A duplicate
    /// delivery yields `Ack::Accepted` (with the original `rev`) and no command
    /// to apply, because the effect is already durable.
    pub fn handle_frame(&mut self, oplog: &OplogService, frame_bytes: &[u8]) -> (Vec<u8>, Option<Command>) {
        let (ack, to_apply) = decode_frame(frame_bytes)
            .map_or_else(|| (reject("", "malformed command frame"), None), |frame| self.process(oplog, frame));
        (encode_ack(&ack), to_apply)
    }

    /// Drive a connected commander: read framed commands until EOF, acking each.
    ///
    /// Supports a long-lived bidirectional connection (the same stream the tee
    /// publishes on, design doc — one UDS consumer per agent). Each fully-read
    /// frame is processed and its ack written back immediately. Returns the
    /// commands that were freshly accepted, in arrival order, for the caller to
    /// apply.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] on a socket read/write fault. A peer that floods
    /// un-decodable bytes past [`MAX_CONNECTION_BUFFER`] ends the connection
    /// with an `Error::Io` rather than growing memory without bound.
    pub fn handle_connection(&mut self, oplog: &OplogService, stream: &mut UnixStream) -> BootResult<Vec<Command>> {
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = vec![0u8; READ_CHUNK].into_boxed_slice();
        let mut applied: Vec<Command> = Vec::new();

        loop {
            // Drain every complete frame currently buffered.
            while let Some((consumed, frame)) = take_frame(&buf) {
                let (ack, to_apply) = self.process(oplog, frame);
                stream.write_all(&encode_ack(&ack)).map_err(|e| Error::io("write command ack", e))?;
                if let Some(command) = to_apply {
                    applied.push(command);
                }
                let _drained: Vec<u8> = buf.drain(..consumed).collect();
            }

            if buf.len() > MAX_CONNECTION_BUFFER {
                return Err(Error::io(
                    "command connection",
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "frame buffer overflow"),
                ));
            }

            let read = stream.read(&mut chunk).map_err(|e| Error::io("read command", e))?;
            if read == 0 {
                return Ok(applied); // EOF: commander closed the connection.
            }
            if let Some(got) = chunk.get(..read) {
                buf.extend_from_slice(got);
            }
        }
    }

    /// Authenticate, dedup, and (if fresh) journal one decoded frame.
    fn process(&mut self, oplog: &OplogService, frame: CommandFrame) -> (Ack, Option<Command>) {
        let command = frame.command;

        // 1. Authenticate the bearer before trusting anything else.
        if frame.auth.is_empty() || frame.auth != self.cap_token {
            return (reject(&command.id, "bad bearer token"), None);
        }

        // 2. Dedup: a token already durable is acknowledged without re-work.
        if self.seen.contains(&command.dedup_token) {
            let rev = self.seen.rev_of(&command.dedup_token);
            return (accept(&command.id, rev), None);
        }

        // 3. Journal-then-ack: durable before accepted. A journal failure is a
        //    rejection (fail-closed) — the commander retries rather than
        //    believing a lost command was applied.
        match oplog.append_durable(OpEntryKind::CommandEffect {
            cmd_id: command.id.clone(),
            dedup_token: command.dedup_token.clone(),
        }) {
            Ok(rev) => {
                self.seen.mark(&command.dedup_token, rev);
                (accept(&command.id, Some(rev)), Some(command))
            }
            Err(e) => (reject(&command.id, &format!("journal failed: {e}")), None),
        }
    }
}

/// Decode a framed payload into a [`CommandFrame`], or `None` if the frame or
/// its JSON is malformed (the caller turns this into a rejection).
fn decode_frame(frame_bytes: &[u8]) -> Option<CommandFrame> {
    let (payload, _consumed) = framing::decode_raw(frame_bytes).ok()?;
    serde_json::from_slice(payload).ok()
}

/// Pull the first complete frame out of `buf`, returning `(bytes_consumed,
/// frame)`, or `None` if the buffer holds no complete, valid frame yet.
///
/// A corrupt/incomplete leading frame returns `None`; the caller keeps reading
/// (an incomplete frame completes later, and a genuinely corrupt one is bounded
/// by [`MAX_CONNECTION_BUFFER`]).
fn take_frame(buf: &[u8]) -> Option<(usize, CommandFrame)> {
    let (payload, consumed) = framing::decode_raw(buf).ok()?;
    let frame = serde_json::from_slice(payload).ok()?;
    Some((consumed, frame))
}

/// Build an `Accepted` ack for `cmd_id` at `rev`.
fn accept(cmd_id: &str, rev: Option<u64>) -> Ack {
    Ack::new(cmd_id.to_owned(), Status::Accepted, rev)
}

/// Build a `Rejected` ack for `cmd_id` with `reason`.
fn reject(cmd_id: &str, reason: &str) -> Ack {
    Ack::new(cmd_id.to_owned(), Status::Rejected { reason: reason.to_owned() }, None)
}

/// Serialise an [`Ack`] and wrap it in the shared length+CRC framing.
///
/// Serialisation of an `Ack` (plain owned strings + an enum) cannot fail; a
/// framing failure (impossibly large) yields an empty buffer, which the peer
/// reads as a closed/again — never a panic.
fn encode_ack(ack: &Ack) -> Vec<u8> {
    serde_json::to_vec(ack).ok().and_then(|p| framing::encode_raw(&p).ok()).unwrap_or_default()
}

/// Flatten a [`cp_oplog::error::Error`] into an [`std::io::Error`] so it can
/// ride a [`Error::Io`].
fn into_io(e: &cp_oplog::error::Error) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_wire::types::command::Kind;
    use tempfile::tempdir;

    const TOKEN: &str = "cap-token-256bit";

    fn frame(auth: &str, dedup: &str) -> Vec<u8> {
        let cf = CommandFrame::new(
            auth.to_owned(),
            Command::new(
                format!("cmd-{dedup}"),
                1,
                dedup.to_owned(),
                Kind::SendMessage { thread_id: "T1".to_owned(), content: "hi".to_owned() },
            ),
        );
        framing::encode_raw(&serde_json::to_vec(&cf).expect("ser")).expect("frame")
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

        let applied = intake.handle_connection(&oplog, &mut server).expect("handle");
        assert_eq!(applied.len(), 1, "one fresh command was accepted off the connection");
        let status = writer.join().expect("join");
        assert_eq!(status, Status::Accepted, "the commander saw an Accepted ack");

        oplog.shutdown().expect("shutdown");
    }
}
