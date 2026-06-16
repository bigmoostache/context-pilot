//! Backend → agent command envelope.
//!
//! A [`Command`] is the unit of work the orchestrator sends to an agent.
//! Every command carries a [`CommandKind`] discriminant, a transport-level
//! `seq` for ordering, and a semantic `dedup_token` for exactly-once
//! idempotency (design doc I4).

use serde::{Deserialize, Serialize};

/// A single command from the backend to an agent.
///
/// Authn is handled at the transport layer (v1: bearer `cap_token` checked
/// before deserialisation reaches this struct — design doc I9).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Command {
    /// Wire-schema revision for this struct (always `1` today).
    pub schema_version: u32,

    /// Transport-level identifier (unique per backend session).
    pub id: String,

    /// Monotonically increasing sequence number for total ordering.
    pub seq: u64,

    /// Semantic dedup key — the oplog's `seen`-set keys on this, not on
    /// `id`, so a TTL-reissue with the same token is deduplicated (I4).
    pub dedup_token: String,

    /// What the command asks the agent to do.
    pub kind: CommandKind,
}

/// The action a [`Command`] requests.
///
/// Uses an internally-tagged representation (`"kind"` field) so that an
/// older receiver encountering a variant it has never seen deserialises it
/// as [`Unknown`](CommandKind::Unknown) instead of failing hard.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum CommandKind {
    /// Send a message to a thread (the primary user action).
    #[serde(rename = "send_message")]
    SendMessage {
        /// Target thread identifier.
        thread_id: String,
        /// Markdown body of the message.
        content: String,
    },

    /// Create a new thread.
    #[serde(rename = "create_thread")]
    CreateThread {
        /// Human-readable thread name.
        name: String,
    },

    /// Archive an existing thread.
    #[serde(rename = "archive_thread")]
    ArchiveThread {
        /// Thread to archive.
        thread_id: String,
    },

    /// Restore a previously archived thread.
    #[serde(rename = "restore_thread")]
    RestoreThread {
        /// Thread to restore.
        thread_id: String,
    },

    /// Interrupt the agent's current LLM stream.
    #[serde(rename = "interrupt_stream")]
    InterruptStream,

    /// Request a graceful stop.
    #[serde(rename = "stop")]
    Stop,

    /// Catch-all for variants added in a newer protocol version.
    ///
    /// An N-1 receiver deserialises any unrecognised `"kind"` tag here
    /// rather than returning a hard error.
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_round_trip() {
        let cmd = Command {
            schema_version: 1,
            id: "cmd-001".into(),
            seq: 42,
            dedup_token: "user-msg-abc".into(),
            kind: CommandKind::SendMessage {
                thread_id: "T1".into(),
                content: "Hello".into(),
            },
        };
        let json = serde_json::to_string(&cmd).expect("serialize");
        let back: Command = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cmd, back);
    }

    #[test]
    fn unknown_kind_tolerant_decode() {
        let json = r#"{
            "schema_version": 1,
            "id": "cmd-002",
            "seq": 1,
            "dedup_token": "dt",
            "kind": {"kind": "fancy_future_thing", "data": 123}
        }"#;
        let cmd: Command = serde_json::from_str(json).expect("tolerant decode");
        assert_eq!(cmd.kind, CommandKind::Unknown);
    }

    #[test]
    fn unknown_extra_fields_ignored() {
        let json = r#"{
            "schema_version": 1,
            "id": "cmd-003",
            "seq": 2,
            "dedup_token": "dt2",
            "kind": {"kind": "stop"},
            "future_field": true
        }"#;
        let cmd: Command = serde_json::from_str(json).expect("ignore unknown fields");
        assert_eq!(cmd.kind, CommandKind::Stop);
    }
}
