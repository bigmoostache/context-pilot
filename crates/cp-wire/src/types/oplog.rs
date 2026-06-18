//! Oplog entry — the authoritative, append-only event record (tier ①).
//!
//! Each [`OpEntry`] is one atomic unit in the oplog WAL.  It carries a
//! monotonic `rev` (the append offset), a wall-clock timestamp for
//! diagnostics, and an [`OpEntryKind`] discriminant describing the event.
//! The oplog is `fsync`'d per commit-group (design doc I2/I8).

use serde::{Deserialize, Serialize};

use super::snapshot::Snapshot;
use super::{ContentHash, LifecycleState, Phase, ThreadTurn};

/// A single oplog record.
///
/// Framed with a length prefix + CRC on the wire (design doc §21 Open-Q1,
/// locked v6); this struct is the *payload* inside that frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpEntry {
    /// Wire-schema revision for this struct.
    pub schema_version: u32,

    /// Monotonic oplog offset — assigned by the single main loop, never
    /// reused, never skipped (design doc I8/K9).
    pub rev: u64,

    /// Wall-clock milliseconds since the Unix epoch (informational — not
    /// load-bearing for ordering; `rev` is the authority).
    pub timestamp_ms: u64,

    /// What happened.
    pub kind: OpEntryKind,
}

/// The event an [`OpEntry`] records.
///
/// Internally tagged by `"kind"`.  An N-1 receiver that encounters a
/// variant it has never seen deserialises it as
/// [`Unknown`](OpEntryKind::Unknown).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum OpEntryKind {
    /// A command was accepted and its effect applied (I6/I11).
    #[serde(rename = "command_effect")]
    CommandEffect {
        /// The transport-level command id that produced this effect.
        cmd_id: String,
        /// Semantic dedup token — the `seen`-set keys on this (I4).
        dedup_token: String,
    },

    /// A dedup token was marked as seen without a separate effect record
    /// (e.g. a duplicate delivery that was already applied).
    #[serde(rename = "seen_mark")]
    SeenMark {
        /// The deduplicated token.
        dedup_token: String,
    },

    /// The agent transitioned to a new execution phase.
    #[serde(rename = "phase_transition")]
    PhaseTransition {
        /// The phase the agent entered.
        phase: Phase,
    },

    /// A new message was created (finalised by the agent).
    #[serde(rename = "message_created")]
    MessageCreated {
        /// Thread the message belongs to.
        thread_id: String,
        /// Unique message identifier.
        message_id: String,
        /// Content-addressed hash of the message body (I3/I13).
        head: ContentHash,
        /// The message body, embedded **inline** when it is small enough to
        /// ride this entry's own `fdatasync` (the common chat case — I13's
        /// inline-small path). The bytes are UTF-8 JSON describing the message
        /// (author, text, timestamp, optional question/file-ref), so an
        /// observer renders the bubble with **zero hydration round-trip**.
        ///
        /// `None` when the body was large enough to **spill** to the
        /// content-addressed body store instead; an observer then hydrates it
        /// by [`head`](Self::MessageCreated::head) over `/body/{hash}`.
        ///
        /// `#[serde(default)]` keeps the field optional on the wire: an N-1
        /// reader that predates it simply sees `None` (spilled-style hydrate),
        /// and it is omitted entirely for spilled bodies.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inline_body: Option<String>,
    },

    /// A new thread was opened.
    ///
    /// Carries enough to materialise the roster entry without a disk read: the
    /// roster is the lightweight per-thread metadata the `/threads` endpoint
    /// serves from the in-memory view (design doc §16 lists thread
    /// create/archive/restore as oplog-journaled actions; I8). Message count
    /// and last-activity are *derived* by folding subsequent
    /// [`MessageCreated`](Self::MessageCreated) entries — they are not repeated
    /// here.
    #[serde(rename = "thread_created")]
    ThreadCreated {
        /// Identifier of the new thread (e.g. `"T7"`).
        thread_id: String,
        /// User-chosen thread label.
        name: String,
        /// Initial turn ownership at creation.
        status: ThreadTurn,
        /// Wall-clock creation time (epoch ms) — seeds the roster's
        /// last-activity until the first message lands.
        timestamp_ms: u64,
    },

    /// A thread was archived (soft-delete — hidden from the active list, kept
    /// for restore).
    #[serde(rename = "thread_archived")]
    ThreadArchived {
        /// The archived thread.
        thread_id: String,
    },

    /// A previously-archived thread was restored to the active list.
    #[serde(rename = "thread_restored")]
    ThreadRestored {
        /// The restored thread.
        thread_id: String,
    },

    /// A thread's turn ownership changed (`MyTurn` ↔ `TheirTurn`).
    #[serde(rename = "thread_status_changed")]
    ThreadStatusChanged {
        /// The affected thread.
        thread_id: String,
        /// The thread's new turn ownership.
        status: ThreadTurn,
    },

    /// Agent lifecycle state changed (boot, shutdown, etc.).
    #[serde(rename = "lifecycle")]
    Lifecycle {
        /// The new lifecycle state.
        state: LifecycleState,
    },

    /// Periodic cost snapshot for the durable `CostBreaker` (R2-8).
    #[serde(rename = "cost_aggregate")]
    CostAggregate {
        /// Cumulative input tokens since agent boot.
        input_tokens: u64,
        /// Cumulative output tokens since agent boot.
        output_tokens: u64,
        /// Cumulative spend in USD since agent boot.
        cost_usd: f64,
    },

    /// State checkpoint — bounds replay length on restart (GAP 1 / I5).
    #[serde(rename = "checkpoint")]
    Checkpoint {
        /// Full recoverable snapshot (heads + seen-set) at this rev.
        snapshot: Snapshot,
    },

    /// Catch-all for variants from a newer protocol version.
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opentry_round_trip() {
        let entry = OpEntry {
            schema_version: 1,
            rev: 17,
            timestamp_ms: 1_718_000_000_000,
            kind: OpEntryKind::PhaseTransition {
                phase: Phase::Streaming,
            },
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: OpEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(entry, back);
    }

    #[test]
    fn message_created_round_trip() {
        let hash = ContentHash::new([0xde; 32]);
        let entry = OpEntry {
            schema_version: 1,
            rev: 42,
            timestamp_ms: 1_718_000_001_000,
            kind: OpEntryKind::MessageCreated {
                thread_id: "T5".into(),
                message_id: "msg-abc".into(),
                head: hash,
                inline_body: None,
            },
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: OpEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(entry, back);
    }

    #[test]
    fn message_created_inline_body_round_trips_and_omits_when_none() {
        let hash = ContentHash::new([0x07; 32]);
        // Inlined body survives the round-trip verbatim.
        let inlined = OpEntry {
            schema_version: 1,
            rev: 7,
            timestamp_ms: 0,
            kind: OpEntryKind::MessageCreated {
                thread_id: "T1".into(),
                message_id: "T1-m0".into(),
                head: hash,
                inline_body: Some(r#"{"author":"user","text":"hi"}"#.into()),
            },
        };
        let json = serde_json::to_string(&inlined).expect("serialize");
        assert!(json.contains("inline_body"), "inline body present on the wire: {json}");
        assert_eq!(serde_json::from_str::<OpEntry>(&json).expect("deserialize"), inlined);

        // A spilled (None) body is omitted from the wire entirely.
        let spilled = OpEntry {
            schema_version: 1,
            rev: 8,
            timestamp_ms: 0,
            kind: OpEntryKind::MessageCreated {
                thread_id: "T1".into(),
                message_id: "T1-m1".into(),
                head: hash,
                inline_body: None,
            },
        };
        let json = serde_json::to_string(&spilled).expect("serialize");
        assert!(!json.contains("inline_body"), "spilled body omits the field: {json}");
        assert_eq!(serde_json::from_str::<OpEntry>(&json).expect("deserialize"), spilled);
    }

    #[test]
    fn unknown_opentry_kind_tolerant() {
        let json = r#"{
            "schema_version": 1,
            "rev": 99,
            "timestamp_ms": 0,
            "kind": {"kind": "future_event", "payload": [1,2,3]}
        }"#;
        let entry: OpEntry = serde_json::from_str(json).expect("tolerant decode");
        assert_eq!(entry.kind, OpEntryKind::Unknown);
    }

    #[test]
    fn thread_roster_kinds_round_trip() {
        let kinds = [
            OpEntryKind::ThreadCreated {
                thread_id: "T7".into(),
                name: "Refactor the cache engine".into(),
                status: ThreadTurn::MyTurn,
                timestamp_ms: 1_718_000_002_000,
            },
            OpEntryKind::ThreadArchived { thread_id: "T7".into() },
            OpEntryKind::ThreadRestored { thread_id: "T7".into() },
            OpEntryKind::ThreadStatusChanged {
                thread_id: "T7".into(),
                status: ThreadTurn::TheirTurn,
            },
        ];
        for kind in kinds {
            let entry = OpEntry { schema_version: 1, rev: 1, timestamp_ms: 0, kind: kind.clone() };
            let json = serde_json::to_string(&entry).expect("serialize");
            let back: OpEntry = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(entry, back);
        }
    }

    #[test]
    fn thread_created_wire_tag_is_stable() {
        // The internally-tagged discriminant is part of the wire contract.
        let entry = OpEntry {
            schema_version: 1,
            rev: 3,
            timestamp_ms: 0,
            kind: OpEntryKind::ThreadArchived { thread_id: "T1".into() },
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(json.contains("\"kind\":\"thread_archived\""), "stable tag: {json}");
    }
}
