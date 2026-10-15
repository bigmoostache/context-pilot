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

use std::os::unix::net::UnixStream;
use std::time::Duration;

use cp_oplog::replay;
use cp_oplog::service::Service as OplogService;
use cp_wire::framing;
use cp_wire::types::ack::{Ack, Status};
use cp_wire::types::command::{Command, Frame as CommandFrame, Kind};
use cp_wire::types::oplog::OpEntryKind;
use cp_wire::types::payload::query::Inbound;
use cp_wire::types::snapshot::SeenSet;

use crate::error::{BootResult, Error};

/// Read-path query answering (bearer check + injected lookup + framing).
pub mod auth;
mod conn;
pub mod reply;
pub mod server;

use conn::Conn;

/// Default whole-connection budget.
///
/// Matches the orchestrator's `ACK_READ_TIMEOUT` (5 s): past it the commander has already given up on
/// the ack, so serving the connection further only wastes the intake thread.
pub const DEFAULT_CONNECTION_BUDGET: Duration = Duration::from_secs(5);

/// What one connection produced: the commands freshly accepted (journaled and
/// acked, so they **must** be applied) and the fault that ended it, if any.
///
/// The commands survive the fault on purpose: a command acked before the peer
/// misbehaved is durable and its commander believes it accepted, so dropping
/// it here would lose an acknowledged effect.
#[derive(Debug, Default)]
pub struct Served {
    /// Fresh commands, in arrival order.
    pub applied: Vec<Command>,
    /// The socket fault or deadline that ended the connection, `None` on EOF.
    pub error: Option<Error>,
}

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

    /// Whole-connection deadline applied by [`serve_connection`](Self::serve_connection).
    connection_budget: Duration,
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
        Ok(Self { cap_token, seen: recovered.seen, connection_budget: DEFAULT_CONNECTION_BUDGET })
    }

    /// Override the whole-connection budget (tests use a short one).
    #[must_use]
    pub const fn with_connection_budget(mut self, budget: Duration) -> Self {
        self.connection_budget = budget;
        self
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

    /// Drive a connected peer: read framed [`Inbound`] frames until EOF,
    /// answering each in place.
    ///
    /// Supports a long-lived bidirectional connection (the same stream the tee
    /// publishes on, design doc — one UDS consumer per agent). Each fully-read
    /// frame is dispatched by shape (T671):
    ///
    /// - a **command** frame runs the unchanged authenticate → dedup →
    ///   journal-then-ack path, and its framed [`Ack`] is written back
    ///   immediately;
    /// - a **query** frame is answered read-only via `responder` (bearer check
    ///   only — no journal, no dedup, nothing to apply) and its framed
    ///   [`Response`](cp_wire::types::payload::query::Response) is written back.
    ///
    /// Returns the commands that were freshly accepted, in arrival order, for
    /// the caller to apply. Queries contribute nothing to that vector — they
    /// mutate nothing.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] on a socket fault, once the connection's budget
    /// ([`DEFAULT_CONNECTION_BUDGET`] unless overridden) runs out, or when a
    /// peer floods un-decodable bytes past the 32 MiB buffer cap. Commands
    /// accepted before the fault are lost to the caller here; use
    /// [`serve_connection`](Self::serve_connection) to keep them.
    pub fn handle_connection(
        &mut self,
        oplog: &OplogService,
        stream: &mut UnixStream,
        responder: reply::Responder<'_>,
    ) -> BootResult<Vec<Command>> {
        let served = self.serve_connection(oplog, stream, responder);
        served.error.map_or(Ok(served.applied), Err)
    }

    /// Like [`handle_connection`](Self::handle_connection), but always returns
    /// the commands accepted so far alongside the fault that ended the
    /// connection, so an acked command is never dropped (see [`Served`]).
    pub fn serve_connection(
        &mut self,
        oplog: &OplogService,
        stream: &mut UnixStream,
        responder: reply::Responder<'_>,
    ) -> Served {
        let mut served = Served::default();
        let mut conn = match Conn::new(stream, self.connection_budget) {
            Ok(c) => c,
            Err(e) => {
                served.error = Some(Error::io("prepare command connection", e));
                return served;
            }
        };
        loop {
            if let Err(e) = self.answer_buffered(&mut conn, (oplog, responder), &mut served.applied) {
                served.error = Some(Error::io("write command reply", e));
                return served;
            }
            let more = {
                let _p = cp_base::perf_span!("bridge_read");
                conn.read_more()
            };
            match more {
                Ok(true) => {}
                Ok(false) => return served, // EOF: commander closed the connection.
                Err(e) => {
                    served.error = Some(Error::io("read command", e));
                    return served;
                }
            }
        }
    }

    /// Answer every complete frame currently buffered in `conn`, pushing fresh
    /// commands onto `applied` before their ack is written.
    fn answer_buffered(
        &mut self,
        conn: &mut Conn<'_>,
        (oplog, responder): (&OplogService, reply::Responder<'_>),
        applied: &mut Vec<Command>,
    ) -> std::io::Result<()> {
        while let Some((consumed, inbound)) = take_inbound(conn.buffered()) {
            let out = match inbound {
                Inbound::Command(frame) => {
                    let (ack, to_apply) = self.process(oplog, frame);
                    applied.extend(to_apply);
                    encode_ack(&ack)
                }
                Inbound::Query(frame) => reply::encode(&reply::answer(&frame, &self.cap_token, responder)),
            };
            conn.consume(consumed);
            let _p = cp_base::perf_span!("bridge_write_reply");
            conn.write(&out)?;
        }
        Ok(())
    }

    /// Authenticate, dedup, and (if fresh) journal one decoded frame.
    fn process(&mut self, oplog: &OplogService, frame: CommandFrame) -> (Ack, Option<Command>) {
        let command = frame.command;

        // 1. Authenticate the bearer before trusting anything else.
        if !auth::bearer_matches(&frame.auth, &self.cap_token) {
            return (reject(&command.id, "bad bearer token"), None);
        }

        // 1b. A kind this agent cannot decode (newer protocol) is never applied,
        //     so it must never be acknowledged as accepted nor journalled.
        if matches!(command.kind, Kind::Unknown) {
            return (reject(&command.id, "unknown command kind (protocol too new for this agent)"), None);
        }

        // 2. Dedup: a token already durable is acknowledged without re-work.
        if self.seen.contains(&command.dedup_token) {
            let rev = self.seen.rev_of(&command.dedup_token);
            return (accept(&command.id, rev), None);
        }

        // 3. Journal-then-ack: durable before accepted. A journal failure is a
        //    rejection (fail-closed) — the commander retries rather than
        //    believing a lost command was applied.
        let journaled = {
            let _p = cp_base::perf_span!("bridge_journal");
            oplog.append_durable(OpEntryKind::CommandEffect {
                cmd_id: command.id.clone(),
                dedup_token: command.dedup_token.clone(),
            })
        };
        match journaled {
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
/// inbound)`, or `None` if the buffer holds no complete, valid frame yet.
///
/// Decodes into [`Inbound`] — the untagged query-or-command discriminator —
/// so one socket carries both planes (T671). The union is tried query-first and
/// the two shapes are disjoint by required field (`query` vs `command`), so a
/// pre-T671 bare command frame still decodes as
/// [`Command`](Inbound::Command): the command path is byte-identical.
///
/// A corrupt/incomplete leading frame returns `None`; the caller keeps reading
/// (an incomplete frame completes later, and a genuinely corrupt one is bounded
/// by [`MAX_CONNECTION_BUFFER`]).
fn take_inbound(buf: &[u8]) -> Option<(usize, Inbound)> {
    let (payload, consumed) = framing::decode_raw(buf).ok()?;
    let inbound = serde_json::from_slice(payload).ok()?;
    Some((consumed, inbound))
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
mod tests;
