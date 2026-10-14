//! Live streaming frame — ephemeral tier ③ traffic.
//!
//! [`Frame`] carries a single token delta, tool-arg chunk, or
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "RawFrame")]
#[expect(
    clippy::exhaustive_structs,
    reason = "stream-frame contract: Frame is a 7-field ephemeral routing record built cross-crate by the agent's tee path; a constructor would take 6 positional arguments (tripping too_many_arguments) with no natural grouping, so #[non_exhaustive] is impossible and the exhaustive literal is the honest shape"
)]
pub struct Frame {
    /// Wire-schema revision for this struct.
    pub schema_version: u32,

    /// Originating fleet (one cpilot process).
    pub fleet_id: String,

    /// Worker within the fleet (future multi-worker; single today).
    pub worker_id: String,

    /// Agent the frame belongs to, for routing to the correct UI pane.
    pub agent_id: String,

    /// Message being streamed (tokens accumulate under this id).
    pub message_id: String,

    /// Per-message monotonic counter — a gap signals a dropped frame.
    pub seq: u64,

    /// What this frame carries.
    pub kind: Kind,
}

/// Decode shape accepting both protocol revisions of [`Frame`].
///
/// v1 keyed the fleet as `agent_id` and the agent as `thread_id`; v2 uses
/// `fleet_id` + `agent_id`. The same `agent_id` key changes meaning between
/// them, so plain serde aliases can't tell them apart — the presence of
/// `fleet_id` does.
#[derive(Deserialize)]
struct RawFrame {
    /// See [`Frame::schema_version`].
    schema_version: u32,
    /// v2 fleet key; absent on v1 frames.
    fleet_id: Option<String>,
    /// v2 agent key, or the v1 fleet key.
    #[serde(default)]
    agent_id: String,
    /// v1 agent key.
    #[serde(default)]
    thread_id: String,
    /// See [`Frame::worker_id`].
    worker_id: String,
    /// See [`Frame::message_id`].
    message_id: String,
    /// See [`Frame::seq`].
    seq: u64,
    /// See [`Frame::kind`].
    kind: Kind,
}

impl From<RawFrame> for Frame {
    fn from(raw: RawFrame) -> Self {
        let (fleet_id, agent_id) = match raw.fleet_id {
            Some(fleet) => (fleet, raw.agent_id),
            None => (raw.agent_id, raw.thread_id),
        };
        Self {
            schema_version: raw.schema_version,
            fleet_id,
            worker_id: raw.worker_id,
            agent_id,
            message_id: raw.message_id,
            seq: raw.seq,
            kind: raw.kind,
        }
    }
}

/// The payload a [`Frame`] delivers.
///
/// Internally tagged by `"kind"` with an `Unknown` catch-all for
/// forward compatibility.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
#[expect(
    clippy::exhaustive_enums,
    reason = "wire-protocol contract: the stream-frame Kind carries an Unknown catch-all for forward-compat; its variant set is otherwise closed and constructed cross-crate (the agent emits frames, the backend fans them out), so #[non_exhaustive] would forbid that construction"
)]
pub enum Kind {
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
        let frame = Frame {
            schema_version: 1,
            fleet_id: "agent-a".into(),
            worker_id: "w0".into(),
            agent_id: "T1".into(),
            message_id: "msg-42".into(),
            seq: 7,
            kind: Kind::Token { text: "Hello".into() },
        };
        let json = serde_json::to_string(&frame).expect("serialize");
        let back: Frame = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(frame, back);
    }

    #[test]
    fn phase_hint_round_trip() {
        let frame = Frame {
            schema_version: 1,
            fleet_id: "a".into(),
            worker_id: "w".into(),
            agent_id: "T".into(),
            message_id: "m".into(),
            seq: 0,
            kind: Kind::PhaseHint { phase: Phase::Tooling },
        };
        let json = serde_json::to_string(&frame).expect("serialize");
        let back: Frame = serde_json::from_str(&json).expect("deserialize");
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
        let frame: Frame = serde_json::from_str(json).expect("tolerant decode");
        assert_eq!(frame.kind, Kind::Unknown);
        assert_eq!((frame.fleet_id.as_str(), frame.agent_id.as_str()), ("a", "T"), "v1 keys map");
    }

    #[test]
    fn v2_keys_on_the_wire() {
        let frame = Frame {
            schema_version: 1,
            fleet_id: "f".into(),
            worker_id: "w".into(),
            agent_id: "A1".into(),
            message_id: "m".into(),
            seq: 0,
            kind: Kind::Unknown,
        };
        let json = serde_json::to_string(&frame).expect("serialize");
        assert!(json.contains("\"fleet_id\":\"f\"") && json.contains("\"agent_id\":\"A1\""), "{json}");
    }
}
