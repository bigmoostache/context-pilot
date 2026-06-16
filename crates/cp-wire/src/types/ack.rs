//! Command acknowledgment — the backend's receipt for a submitted command.
//!
//! An [`Ack`] tells the sender whether its [`Command`](super::Command) was
//! durably accepted (journal-then-ack, design doc I11) or rejected, and if
//! accepted, which `rev` it landed at.

use serde::{Deserialize, Serialize};

/// Acknowledgment for a single command.
///
/// "Accepted" means **durable**: the command's effect is in the fsync'd
/// oplog before this ack is sent (I11).  A deadman re-exec replays it
/// exactly once (I4/K2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ack {
    /// Wire-schema revision for this struct.
    pub schema_version: u32,

    /// The transport-level command id being acknowledged.
    pub cmd_id: String,

    /// Whether the command was accepted or rejected.
    pub status: Status,

    /// The oplog `rev` at which the effect landed (only present on accept).
    pub rev: Option<u64>,
}

/// Outcome of command processing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum Status {
    /// Command durably accepted — effect is in the oplog.
    #[serde(rename = "accepted")]
    Accepted,

    /// Command rejected (bad auth, unknown agent, validation failure, …).
    #[serde(rename = "rejected")]
    Rejected {
        /// Human-readable reason for the rejection.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ack_accepted_round_trip() {
        let ack = Ack {
            schema_version: 1,
            cmd_id: "cmd-99".into(),
            status: Status::Accepted,
            rev: Some(42),
        };
        let json = serde_json::to_string(&ack).expect("serialize");
        let back: Ack = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(ack, back);
    }

    #[test]
    fn ack_rejected_round_trip() {
        let ack = Ack {
            schema_version: 1,
            cmd_id: "cmd-100".into(),
            status: Status::Rejected {
                reason: "bad bearer token".into(),
            },
            rev: None,
        };
        let json = serde_json::to_string(&ack).expect("serialize");
        let back: Ack = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(ack, back);
    }

    #[test]
    fn ack_extra_fields_ignored() {
        let json = r#"{
            "schema_version": 1,
            "cmd_id": "cmd-1",
            "status": {"status": "accepted"},
            "rev": 5,
            "future_field": "ignored"
        }"#;
        let ack: Ack = serde_json::from_str(json).expect("tolerant decode");
        assert_eq!(ack.status, Status::Accepted);
    }
}
