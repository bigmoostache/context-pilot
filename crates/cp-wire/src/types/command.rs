//! Backend → agent command envelope.
//!
//! A [`Command`] is the unit of work the orchestrator sends to an agent.
//! Every command carries a [`Kind`] discriminant, a transport-level
//! `seq` for ordering, and a semantic `dedup_token` for exactly-once
//! idempotency (design doc I4).

use serde::{Deserialize, Serialize};

/// A single command from the backend to an agent.
///
/// Authn is handled at the transport layer (v1: bearer `cap_token` checked
/// before deserialisation reaches this struct — design doc I9).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    pub kind: Kind,
}

/// The action a [`Command`] requests.
///
/// Uses an internally-tagged representation (`"kind"` field) so that an
/// older receiver encountering a variant it has never seen deserialises it
/// as [`Unknown`](Kind::Unknown) instead of failing hard.

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Kind {
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

    /// Pause a thread (suppress `MY_TURN` notifications).
    #[serde(rename = "pause_thread")]
    PauseThread {
        /// Thread to pause.
        thread_id: String,
    },

    /// Resume a previously paused thread.
    #[serde(rename = "resume_thread")]
    ResumeThread {
        /// Thread to resume.
        thread_id: String,
    },

    /// Interrupt the agent's current LLM stream.
    #[serde(rename = "interrupt_stream")]
    InterruptStream,

    /// Request a graceful stop.
    #[serde(rename = "stop")]
    Stop,

    /// Change agent LLM configuration (provider + model).
    #[serde(rename = "configure")]
    Configure {
        /// LLM provider serde name (e.g. `"anthropic"`, `"claudecodev2"`).
        provider: String,
        /// Model serde name within that provider (e.g. `"claude-opus45"`).
        model: String,
    },

    /// Catch-all for variants added in a newer protocol version.
    ///
    /// An N-1 receiver deserialises any unrecognised `"kind"` tag here
    /// rather than returning a hard error.
    #[serde(other)]
    Unknown,
}

/// Transport envelope carrying a [`Command`] plus its bearer credential.
///
/// The command's authn lives **outside** the [`Command`] struct, in this
/// envelope, so a receiver checks the bearer before trusting the payload
/// (design doc I9). For v1 the credential is the agent's `cap_token` (a
/// presence-checked bearer secret read from the `0600` registry file); the
/// remote-transport seam (G7) later wraps this same envelope in an HMAC/nonce
/// without changing the shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    /// Wire-schema revision for this struct.
    pub schema_version: u32,

    /// Bearer credential — the agent's `cap_token`. An empty string is a
    /// missing bearer and is always rejected.
    pub auth: String,

    /// The command to apply once the bearer is verified.
    pub command: Command,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_frame_round_trip() {
        let frame = Frame {
            schema_version: 1,
            auth: "tok-secret".into(),
            command: Command {
                schema_version: 1,
                id: "cmd-1".into(),
                seq: 1,
                dedup_token: "dt-1".into(),
                kind: Kind::Stop,
            },
        };
        let json = serde_json::to_string(&frame).expect("serialize");
        let back: Frame = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(frame, back);
    }

    #[test]
    fn command_round_trip() {
        let cmd = Command {
            schema_version: 1,
            id: "cmd-001".into(),
            seq: 42,
            dedup_token: "user-msg-abc".into(),
            kind: Kind::SendMessage { thread_id: "T1".into(), content: "Hello".into() },
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
        assert_eq!(cmd.kind, Kind::Unknown);
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
        assert_eq!(cmd.kind, Kind::Stop);
    }

    #[test]
    fn configure_round_trip() {
        let cmd = Command {
            schema_version: 1,
            id: "cmd-004".into(),
            seq: 5,
            dedup_token: "cfg-1".into(),
            kind: Kind::Configure { provider: "anthropic".into(), model: "claude-opus45".into() },
        };
        let json = serde_json::to_string(&cmd).expect("serialize");
        let back: Command = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cmd, back);
    }
}
