//! Oplog entry — the authoritative, append-only event record (tier ①).
//!
//! Each [`OpEntry`] is one atomic unit in the oplog WAL.  It carries a
//! monotonic `rev` (the append offset), a wall-clock timestamp for
//! diagnostics, and an [`OpEntryKind`] discriminant describing the event.
//! The oplog is `fsync`'d per commit-group (design doc I2/I8).

use serde::{Deserialize, Serialize};

use super::{ContentHash, Heads, LifecycleState, Phase};

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

    /// Heads checkpoint — bounds replay length on restart (GAP 1 / I5).
    #[serde(rename = "checkpoint")]
    Checkpoint {
        /// Snapshot of current heads at this rev.
        heads: Heads,
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
            },
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: OpEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(entry, back);
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
}
