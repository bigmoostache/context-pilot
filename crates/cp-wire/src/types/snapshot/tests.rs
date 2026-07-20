//! Roster-fold tests (extracted from `snapshot.rs` for the 500-line cap).
//!
//! Loaded via `#[path = "tests.rs"] mod tests;` in `snapshot/mod.rs`, so
//! `super` resolves to the snapshot module (where `Heads`, `SeenSet`, etc.
//! live). No inner `mod tests { .. }` wrapper — that would double-nest and
//! break the `use super::*` import.

use super::*;

#[test]
fn heads_round_trip() {
    let heads = Heads {
        schema_version: 1,
        threads: vec![ThreadHead { thread_id: "T1".into(), last_message_hash: ContentHash::new([0x11; 32]) }],
        panels: vec![PanelHead { panel_id: "P5".into(), hash: ContentHash::new([0x22; 32]) }],
    };
    let json = serde_json::to_string(&heads).expect("serialize");
    let back: Heads = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(heads, back);
}

#[test]
fn empty_heads_round_trip() {
    let heads = Heads::default();
    let json = serde_json::to_string(&heads).expect("serialize");
    let back: Heads = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(heads, back);
}

#[test]
fn seen_set_marks_and_contains() {
    let mut seen = SeenSet::default();
    assert!(!seen.contains("cmd-a"));
    seen.mark("cmd-a", 3);
    assert!(seen.contains("cmd-a"));
    assert!(!seen.contains("cmd-b"));
}

#[test]
fn seen_set_mark_is_idempotent_and_keeps_first_rev() {
    let mut seen = SeenSet::default();
    seen.mark("cmd-a", 3);
    seen.mark("cmd-a", 9); // duplicate delivery at a later rev
    assert_eq!(seen.len(), 1, "duplicate token must not add a second entry");
    assert_eq!(seen.entries.first().expect("entry").rev, 3, "first rev is kept");
}

#[test]
fn seen_set_rev_of_returns_first_commit_rev() {
    let mut seen = SeenSet::default();
    assert_eq!(seen.rev_of("cmd-a"), None, "absent token has no rev");
    seen.mark("cmd-a", 3);
    seen.mark("cmd-a", 9);
    assert_eq!(seen.rev_of("cmd-a"), Some(3), "rev_of returns the first-commit rev");
}

#[test]
fn seen_set_eviction_is_rev_based_not_time_based() {
    // V3: a token survives an arbitrarily long outage because eviction is
    // gated on acknowledged-rev, never wall-clock. We never advance the
    // ack barrier past the token's rev, so it stays — regardless of how
    // much (simulated) time elapsed.
    let mut seen = SeenSet::default();
    seen.mark("cmd-late", 10);
    seen.evict_through(9); // backend has only acked through rev 9
    assert!(seen.contains("cmd-late"), "un-acked token survives any time gap");
    seen.evict_through(10); // now its rev is acknowledged
    assert!(!seen.contains("cmd-late"), "acked token is evicted");
}

#[test]
fn snapshot_round_trip() {
    let mut snapshot = Snapshot::default();
    snapshot.heads.set_thread_head("T1", ContentHash::new([0x33; 32]));
    snapshot.seen.mark("cmd-x", 5);
    let json = serde_json::to_string(&snapshot).expect("serialize");
    let back: Snapshot = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(snapshot, back);
}

#[test]
fn snapshot_round_trip_with_roster() {
    let mut snapshot = Snapshot::default();
    snapshot.heads.set_thread_head("T1", ContentHash::new([0x33; 32]));
    snapshot.seen.mark("cmd-x", 5);
    snapshot.roster.push(RosterThread {
        thread_id: "T1".into(),
        name: "Plan".into(),
        status: ThreadTurn::MyTurn,
        archived: false,
        paused: false,
        last_activity_ms: 1_700,
        msg_count: 3,
    });
    let json = serde_json::to_string(&snapshot).expect("serialize");
    let back: Snapshot = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(snapshot, back);
}

#[test]
fn snapshot_without_roster_field_decodes_to_empty() {
    // N-1: a checkpoint serialised by an older agent (no `roster` key) must
    // decode with an empty roster rather than failing.
    let legacy = r#"{
        "heads": {"schema_version": 1, "threads": [], "panels": []},
        "seen": {"schema_version": 1, "entries": []}
    }"#;
    let snapshot: Snapshot = serde_json::from_str(legacy).expect("tolerant decode");
    assert!(snapshot.roster.is_empty(), "missing roster field defaults to empty");
}

#[test]
fn roster_fold_helpers_insert_update_and_accumulate() {
    let mut roster: Vec<RosterThread> = Vec::new();
    RosterThread::fold_created(
        &mut roster,
        ThreadCreation { thread_id: "T1", name: "Plan", status: ThreadTurn::TheirTurn, timestamp_ms: 100 },
    );
    assert_eq!(roster.len(), 1);
    // A re-seen creation refreshes, never duplicates.
    RosterThread::fold_created(
        &mut roster,
        ThreadCreation { thread_id: "T1", name: "Plan v2", status: ThreadTurn::MyTurn, timestamp_ms: 100 },
    );
    assert_eq!(roster.len(), 1, "duplicate creation folds idempotently");
    assert_eq!(roster[0].name, "Plan v2");
    assert_eq!(roster[0].status, ThreadTurn::MyTurn);

    RosterThread::fold_message(&mut roster, "T1", 250);
    RosterThread::fold_message(&mut roster, "T1", 400);
    assert_eq!(roster[0].msg_count, 2);
    assert_eq!(roster[0].last_activity_ms, 400, "activity tracks the latest message");

    RosterThread::fold_archived(&mut roster, "T1", true);
    assert!(roster[0].archived);
    RosterThread::fold_archived(&mut roster, "T1", false);
    assert!(!roster[0].archived);

    // Folds for an unknown thread are no-ops, never a panic.
    RosterThread::fold_message(&mut roster, "T-absent", 999);
    RosterThread::fold_status(&mut roster, "T-absent", ThreadTurn::MyTurn);
    assert_eq!(roster.len(), 1);
}
