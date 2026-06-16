//! Live streaming frame — ephemeral tier ③ traffic.
//!
//! [`StreamFrame`] carries a single token delta, tool-arg chunk, or
//! latency hint from the agent to the backend over the UDS stream plane.
//! Frames are best-effort and droppable (design doc I7); the oplog is the
//! safety net for any dropped hint.

use serde::{Deserialize, Serialize};

use super::Phase;

/// One frame on the ephemeral stream plane.
///
/// The `seq` is **per-`message_id`** so gaps are unambiguous (design doc
/// I10).  The backend fans these out to N frontend WebSocket subscribers
/// without touching the agent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StreamFrame {
    /// Wire-schema revision for this struct.
    pub schema_version: u32,

    /// Originating agent.
    pub agent_id: String,

    /// Worker within the agent (future multi-worker; single today).
    pub worker_id: String,

    /// Thread context for routing to the correct UI pane.
    pub thread_id: String,

    /// Message being streamed (tokens accumulate under this id).
    pub message_id: String,

    /// Per-message monotonic counter — a gap signals a dropped frame.
    pub seq: u64,

    /// What this frame carries.
    pub kind: StreamKind,
}

/// The payload a [`StreamFrame`] delivers.
///
/// Internally tagged by `"kind"` with an `Unknown` catch-all for
/// forward compatibility.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum StreamKind {
    /// Latency hint that a new message is starting (self-describing so the
    /// frontend can paint before the oplog entry lands).
    #[serde(rename = "message_start_hint")]
    MessageStartHint {
        /// The role of the message (e.g. `"assistant"`).
        role: String,
    },

    /// A chunk of LLM-generated text.
    #[serde(rename = "token")]
    Token {
        /// The text fragment.
        text: String,
    },

    /// A chunk of tool-call argument JSON.
    #[serde(rename = "tool_args")]
    ToolArgs {
        /// Which tool use this chunk belongs to.
        tool_use_id: String,
        /// Partial JSON fragment.
        json_chunk: String,
    },

    /// Latency hint for a phase change (truth is the oplog, I10/K6).
    #[serde(rename = "phase_hint")]
    PhaseHint {
        /// The phase the agent just entered.
        phase: Phase,
    },

    /// Catch-all for future frame kinds.
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_frame_round_trip() {
        let frame = StreamFrame {
            schema_version: 1,
            agent_id: "agent-a".into(),
            worker_id: "w0".into(),
            thread_id: "T1".into(),
            message_id: "msg-42".into(),
            seq: 7,
            kind: StreamKind::Token {
                text: "Hello".into(),
            },
        };
        let json = serde_json::to_string(&frame).expect("serialize");
        let back: StreamFrame = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(frame, back);
    }

    #[test]
    fn phase_hint_round_trip() {
        let frame = StreamFrame {
            schema_version: 1,
            agent_id: "a".into(),
            worker_id: "w".into(),
            thread_id: "T".into(),
            message_id: "m".into(),
            seq: 0,
            kind: StreamKind::PhaseHint {
                phase: Phase::Tooling,
            },
        };
        let json = serde_json::to_string(&frame).expect("serialize");
        let back: StreamFrame = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(frame, back);
    }

    #[test]
    fn unknown_stream_kind_tolerant() {
        let json = r#"{
            "schema_version": 1,
            "agent_id": "a",
            "worker_id": "w",
            "thread_id": "T",
            "message_id": "m",
            "seq": 0,
            "kind": {"kind": "future_stream_thing"}
        }"#;
        let frame: StreamFrame = serde_json::from_str(json).expect("tolerant decode");
        assert_eq!(frame.kind, StreamKind::Unknown);
    }
}
