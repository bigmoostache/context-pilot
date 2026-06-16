//! Context Pilot **oplog** — the agent's authoritative, append-only,
//! `fsync`'d write-ahead log (durability tier ①).
//!
//! This crate owns the *file-level* machinery for the operation log that the
//! orchestration design (`docs/design-orchestration-backend.md`) calls the
//! keystone: a tiny, append-only, group-committed log of the handful of events
//! that must be **exactly-once and durable** (command effects, `rev`
//! assignment, `seen`-marks, phase/lifecycle transitions, the cost aggregate,
//! and head checkpoints). The large, churny agent state stays in the existing
//! best-effort `PersistenceWriter` and is treated as a *materialized view* that
//! can be rebuilt by replaying this log.
//!
//! # Why a separate crate
//!
//! The oplog is **shared infrastructure**, not agent- or backend-specific:
//!
//! * the **agent** (`cp-mod-bridge`) *writes* it, and *replays* its own log
//!   after a deadman re-exec to re-derive in-flight effects (I11);
//! * the **backend** (`cp-orchestrator`) *reads/tails* it as the source of
//!   truth for a fleet of agents (I12).
//!
//! Both sides must agree byte-for-byte on the format, so it lives in exactly
//! **one** place. The crate depends only on [`cp_wire`] (for the [`OpEntry`]
//! type and its framing) and the standard library — no UI, no async runtime.
//!
//! [`OpEntry`]: cp_wire::types::oplog::OpEntry
//!
//! # Durability contract
//!
//! Each record is framed (length prefix + CRC-32C, see [`cp_wire::framing`])
//! and appended to the current segment. [`append::OplogWriter::append`] does
//! not return a `rev` until the bytes are durable (`fdatasync`), so an
//! announced `rev` always survives a crash — *announce-after-durable* (design
//! doc K9). A torn tail (a partial write interrupted by power loss) is detected
//! on the next open by a CRC/length failure and truncated away, so the log
//! always reopens at a clean record boundary (V1).
//!
//! *Phase 4–7* implement the writer, segment reading + torn-tail recovery,
//! replay-to-heads, the dedup `seen`-set, and segment compaction + the
//! orphan-body GC grace rule. The off-loop group-commit thread and the bridge
//! body store arrive in later phases.

pub mod append;
pub mod compact;
pub mod replay;
pub mod segment;
