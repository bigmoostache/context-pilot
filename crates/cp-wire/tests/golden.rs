//! Golden cross-version fixtures: hand-written JSON as an **older** or **newer**
//! peer would put it on the wire, decoded by this build.
//!
//! Same-build round-trips cannot catch a compat break (both sides change in
//! lockstep). These literals are frozen: if one stops decoding, a deployed peer
//! stops talking to us. Edit a fixture only for a deliberate major bump.

// Only `serde_json` and `cp_wire` are used directly; the canonical `as _`
// acknowledgement keeps the per-target `unused-crate-dependencies` lint quiet.
use cp_macros as _;
use crc32c as _;
use serde as _;
use sha2 as _;
#[cfg(feature = "openapi")]
use utoipa as _;

#[cfg(test)]
mod tests {
    use cp_wire::types::ack::{Ack, Status};
    use cp_wire::types::command::{Frame as CommandFrame, Kind as CommandKind};
    use cp_wire::types::oplog::{OpEntry, OpEntryKind};
    use cp_wire::types::registry::{Entry, FleetStatus};
    use cp_wire::types::stream::{Frame as StreamFrame, Kind as StreamKind};
    use cp_wire::types::{LifecycleState, Phase};

    /// A registry record written by a pre-T713 agent (no `tee_socket_path`).
    const ENTRY_V1_PRE_T713: &str = r#"{
        "schema_version": 1, "id": "abc", "folder": "/p", "pid": 7, "boot_id": "b",
        "model": "m", "protocol_version": 1, "binary_version": "0.2.10",
        "socket_path": "/p/s.sock", "oplog_path": "/p/oplog", "heartbeat_path": "/p/hb",
        "cap_token": "t", "started_at_ms": 1, "status": "running"
    }"#;

    /// A registry record from a newer agent: unknown status + unknown field.
    const ENTRY_NEWER: &str = r#"{
        "schema_version": 2, "id": "abc", "folder": "/p", "pid": 7, "boot_id": "b",
        "model": "m", "protocol_version": 2, "binary_version": "9.9.9",
        "socket_path": "/p/s.sock", "oplog_path": "/p/oplog", "heartbeat_path": "/p/hb",
        "tee_socket_path": "/p/tee.sock", "cap_token": "t", "started_at_ms": 1,
        "status": "hibernating", "gpu": "h100"
    }"#;

    #[test]
    fn pre_t713_entry_decodes_with_empty_tee_path() {
        let entry: Entry = serde_json::from_str(ENTRY_V1_PRE_T713).expect("older entry must decode");
        assert_eq!(entry.tee_socket_path, "");
        assert_eq!(entry.status, FleetStatus::Running);
    }

    #[test]
    fn newer_entry_with_unknown_status_still_decodes() {
        let entry: Entry = serde_json::from_str(ENTRY_NEWER).expect("newer entry must decode");
        assert_eq!(entry.status, FleetStatus::Unknown);
    }

    #[test]
    fn legacy_thread_command_frame_decodes() {
        let json = r#"{"schema_version":1,"auth":"t","command":{"schema_version":1,"id":"c1","seq":1,
            "dedup_token":"d1","kind":{"kind":"send_message","thread_id":"T1","content":"hi"}}}"#;
        let frame: CommandFrame = serde_json::from_str(json).expect("legacy thread_id alias");
        assert!(matches!(&frame.command.kind, CommandKind::SendMessage { agent_id, .. } if agent_id == "T1"));
    }

    #[test]
    fn newer_command_kind_decodes_to_unknown() {
        let json = r#"{"schema_version":1,"auth":"t","command":{"schema_version":3,"id":"c1","seq":1,
            "dedup_token":"d1","kind":{"kind":"teleport","to":"mars"}}}"#;
        let frame: CommandFrame = serde_json::from_str(json).expect("newer kind must decode");
        assert!(matches!(frame.command.kind, CommandKind::Unknown));
    }

    #[test]
    fn newer_phase_and_lifecycle_values_decode_to_unknown() {
        let phase =
            r#"{"schema_version":1,"rev":3,"timestamp_ms":0,"kind":{"kind":"phase_transition","phase":"compacting"}}"#;
        let entry: OpEntry = serde_json::from_str(phase).expect("newer phase must not drop the entry");
        assert!(matches!(entry.kind, OpEntryKind::PhaseTransition { phase: Phase::Unknown }));

        let life = r#"{"schema_version":1,"rev":4,"timestamp_ms":0,"kind":{"kind":"lifecycle","state":"migrating"}}"#;
        let life_entry: OpEntry = serde_json::from_str(life).expect("newer lifecycle must not drop the entry");
        assert!(matches!(life_entry.kind, OpEntryKind::Lifecycle { state: LifecycleState::Unknown }));
    }

    #[test]
    fn newer_oplog_kind_decodes_to_unknown() {
        let json = r#"{"schema_version":2,"rev":9,"timestamp_ms":0,"kind":{"kind":"hologram","x":1}}"#;
        let entry: OpEntry = serde_json::from_str(json).expect("newer oplog kind must decode");
        assert_eq!(entry.kind, OpEntryKind::Unknown);
    }

    #[test]
    fn newer_phase_hint_stream_frame_decodes() {
        let json = r#"{"schema_version":1,"fleet_id":"f","worker_id":"w","agent_id":"a","message_id":"m",
            "seq":1,"kind":{"kind":"phase_hint","phase":"dreaming"},"extra":true}"#;
        let frame: StreamFrame = serde_json::from_str(json).expect("newer phase hint must decode");
        assert!(matches!(frame.kind, StreamKind::PhaseHint { phase: Phase::Unknown }));
    }

    #[test]
    fn newer_ack_status_is_never_accepted() {
        let json = r#"{"schema_version":2,"cmd_id":"c","status":{"status":"queued"},"rev":null}"#;
        let ack: Ack = serde_json::from_str(json).expect("newer ack must decode");
        assert_eq!(ack.status, Status::Unknown);
    }
}
