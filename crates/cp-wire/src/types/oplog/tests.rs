//! Round-trip and tag-stability tests for [`super`]'s oplog types.
//!
//! Split from `oplog.rs` for the 500-line cap; `#[path]`-included so `super`
//! resolves to the oplog module.

use super::*;

#[test]
fn opentry_round_trip() {
    let entry = OpEntry {
        schema_version: 1,
        rev: 17,
        timestamp_ms: 1_718_000_000_000,
        kind: OpEntryKind::PhaseTransition { phase: Phase::Streaming },
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
        OpEntryKind::ThreadStatusChanged { thread_id: "T7".into(), status: ThreadTurn::TheirTurn },
    ];
    for kind in kinds {
        let entry = OpEntry { schema_version: 1, rev: 1, timestamp_ms: 0, kind: kind.clone() };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: OpEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(entry, back);
    }
}

#[test]
fn context_usage_round_trip_and_stable_tag() {
    let entry = OpEntry {
        schema_version: 1,
        rev: 11,
        timestamp_ms: 0,
        kind: OpEntryKind::ContextUsage {
            used_tokens: 167_766,
            threshold_tokens: 190_000,
            budget_tokens: 200_000,
            hit_tokens: 120_000,
            miss_tokens: 47_766,
        },
    };
    let json = serde_json::to_string(&entry).expect("serialize");
    assert!(json.contains("\"kind\":\"context_usage\""), "stable tag: {json}");
    let back: OpEntry = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(entry, back);
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

#[test]
fn behaviour_changed_round_trip_and_stable_tag() {
    // Carries the new active agent id and round-trips; the id is omitted on
    // a revert-to-default (None).
    let with_id = OpEntry {
        schema_version: 1,
        rev: 5,
        timestamp_ms: 0,
        kind: OpEntryKind::BehaviourChanged { agent_id: Some("caveman".into()) },
    };
    let json = serde_json::to_string(&with_id).expect("serialize");
    assert!(json.contains("\"kind\":\"behaviour_changed\""), "stable tag: {json}");
    assert!(json.contains("caveman"), "carries the active id: {json}");
    assert_eq!(serde_json::from_str::<OpEntry>(&json).expect("deserialize"), with_id);

    let reverted =
        OpEntry { schema_version: 1, rev: 6, timestamp_ms: 0, kind: OpEntryKind::BehaviourChanged { agent_id: None } };
    let json = serde_json::to_string(&reverted).expect("serialize");
    assert!(!json.contains("agent_id"), "None id omitted: {json}");
    assert_eq!(serde_json::from_str::<OpEntry>(&json).expect("deserialize"), reverted);
}

#[test]
fn identity_changed_round_trip_and_stable_tag() {
    let entry = OpEntry { schema_version: 1, rev: 9, timestamp_ms: 0, kind: OpEntryKind::IdentityChanged };
    let json = serde_json::to_string(&entry).expect("serialize");
    assert!(json.contains("\"kind\":\"identity_changed\""), "stable tag: {json}");
    let back: OpEntry = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(entry, back);
}

#[test]
fn task_list_changed_round_trip_and_stable_tag() {
    use super::super::snapshot::todo::{WireTask, WireTaskStatus};
    let entry = OpEntry {
        schema_version: 1,
        rev: 21,
        timestamp_ms: 0,
        kind: OpEntryKind::TaskListChanged {
            thread_id: "T7".into(),
            tasks: vec![
                WireTask {
                    id: "X1".into(),
                    parent_id: None,
                    name: "Root task".into(),
                    description: "the parent".into(),
                    status: WireTaskStatus::InProgress,
                },
                WireTask {
                    id: "X2".into(),
                    parent_id: Some("X1".into()),
                    name: "Child".into(),
                    description: String::new(),
                    status: WireTaskStatus::Done,
                },
            ],
        },
    };
    let json = serde_json::to_string(&entry).expect("serialize");
    assert!(json.contains("\"kind\":\"task_list_changed\""), "stable tag: {json}");
    let back: OpEntry = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(entry, back);
}
