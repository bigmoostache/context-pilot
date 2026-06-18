//! Unit tests for [`super`] (the materialized view fold).
//!
//! Split from `materialized_view.rs` to respect the 500-line structure limit;
//! `#[path]`-included so `super` resolves to the `materialized_view` module.

use super::*;
use cp_wire::types::snapshot::Snapshot;
use cp_wire::types::{ContentHash, ThreadTurn};

/// Build an [`OpEntry`] with the given rev and kind.
fn entry(rev: u64, kind: OpEntryKind) -> OpEntry {
    OpEntry { schema_version: 1, rev, timestamp_ms: 0, kind }
}

fn message(thread: &str, byte: u8) -> OpEntryKind {
    OpEntryKind::MessageCreated {
        thread_id: thread.to_owned(),
        message_id: format!("m{byte}"),
        head: ContentHash::new([byte; 32]),
        inline_body: None,
    }
}

#[test]
fn message_created_sets_thread_head() {
    let mut view = MaterializedView::new();
    view.apply("a1", &entry(0, message("T1", 0x11)));

    let agent = view.get("a1").expect("agent present");
    assert_eq!(agent.rev, 0);
    let head = agent
        .heads
        .threads
        .iter()
        .find(|h| h.thread_id == "T1")
        .expect("thread head present");
    assert_eq!(head.last_message_hash, ContentHash::new([0x11; 32]));
}

#[test]
fn checkpoint_resets_heads_authoritatively() {
    let mut view = MaterializedView::new();
    // Stale head folded first.
    view.apply("a1", &entry(0, message("T1", 0x11)));

    // A checkpoint carrying a different head set must replace, not merge.
    let mut snapshot = Snapshot::default();
    snapshot.heads.set_thread_head("T2", ContentHash::new([0x22; 32]));
    view.apply("a1", &entry(5, OpEntryKind::Checkpoint { snapshot }));

    let agent = view.get("a1").expect("agent present");
    assert_eq!(agent.rev, 5);
    assert!(
        agent.heads.threads.iter().all(|h| h.thread_id != "T1"),
        "checkpoint must drop the pre-checkpoint head",
    );
    assert_eq!(agent.heads.threads.len(), 1);
    assert_eq!(agent.heads.threads.first().expect("T2").thread_id, "T2");
}

#[test]
fn checkpoint_restores_roster_wholesale() {
    let mut view = MaterializedView::new();
    // A stale roster entry folded from a pre-checkpoint delta.
    view.apply(
        "a1",
        &entry(
            0,
            OpEntryKind::ThreadCreated {
                thread_id: "T-stale".into(),
                name: "stale".into(),
                status: ThreadTurn::TheirTurn,
                timestamp_ms: 1,
            },
        ),
    );

    // A checkpoint carrying a *different* roster must replace, not merge —
    // this is the cold-restart-after-compaction path (I5): the backend
    // rebuilds the thread list from the snapshot alone.
    let mut snapshot = Snapshot::default();
    snapshot.roster.push(RosterEntry {
        thread_id: "T7".into(),
        name: "carried".into(),
        status: ThreadTurn::MyTurn,
        archived: true,
        last_activity_ms: 9_000,
        msg_count: 4,
    });
    view.apply("a1", &entry(5, OpEntryKind::Checkpoint { snapshot }));

    let agent = view.get("a1").expect("agent present");
    assert_eq!(agent.roster.len(), 1, "checkpoint replaces the roster wholesale");
    let e = agent.roster.first().expect("entry");
    assert_eq!(e.thread_id, "T7");
    assert_eq!(e.name, "carried");
    assert_eq!(e.status, ThreadTurn::MyTurn);
    assert!(e.archived);
    assert_eq!(e.msg_count, 4);
    assert!(
        agent.roster.iter().all(|r| r.thread_id != "T-stale"),
        "the pre-checkpoint roster entry is dropped",
    );
}

#[test]
fn roster_survives_compaction_via_checkpoint_then_folds_tail() {
    // The full cold-restart shape: a checkpoint seeds the roster, then the
    // post-checkpoint delta tail (a new thread + a message) folds on top.
    let mut view = MaterializedView::new();
    let mut snapshot = Snapshot::default();
    snapshot.roster.push(RosterEntry {
        thread_id: "T1".into(),
        name: "carried".into(),
        status: ThreadTurn::TheirTurn,
        archived: false,
        last_activity_ms: 100,
        msg_count: 2,
    });
    view.apply("a1", &entry(10, OpEntryKind::Checkpoint { snapshot }));
    view.apply(
        "a1",
        &entry(
            11,
            OpEntryKind::ThreadCreated {
                thread_id: "T2".into(),
                name: "fresh".into(),
                status: ThreadTurn::MyTurn,
                timestamp_ms: 200,
            },
        ),
    );
    let mut msg = entry(12, message("T1", 0x01));
    msg.timestamp_ms = 300;
    view.apply("a1", &msg);

    let agent = view.get("a1").expect("agent present");
    assert_eq!(agent.roster.len(), 2, "carried + fresh");
    let t1 = agent.roster.iter().find(|r| r.thread_id == "T1").expect("T1");
    assert_eq!(t1.msg_count, 3, "tail message bumped the carried count");
    assert_eq!(t1.last_activity_ms, 300);
}

#[test]
fn phase_and_lifecycle_are_latest_wins() {
    let mut view = MaterializedView::new();
    view.apply("a1", &entry(0, OpEntryKind::PhaseTransition { phase: Phase::Streaming }));
    view.apply("a1", &entry(1, OpEntryKind::PhaseTransition { phase: Phase::Tooling }));
    view.apply("a1", &entry(2, OpEntryKind::Lifecycle { state: LifecycleState::Running }));

    let agent = view.get("a1").expect("agent present");
    assert_eq!(agent.phase, Some(Phase::Tooling));
    assert_eq!(agent.lifecycle, Some(LifecycleState::Running));
}

#[test]
fn cost_aggregate_is_latest_not_summed() {
    let mut view = MaterializedView::new();
    view.apply(
        "a1",
        &entry(0, OpEntryKind::CostAggregate { input_tokens: 100, output_tokens: 10, cost_usd: 1.0 }),
    );
    view.apply(
        "a1",
        &entry(1, OpEntryKind::CostAggregate { input_tokens: 250, output_tokens: 30, cost_usd: 2.5 }),
    );

    let agent = view.get("a1").expect("agent present");
    // Cumulative-since-boot ⇒ latest wins, never 350/40/3.5.
    assert_eq!(agent.cost.input_tokens, 250);
    assert_eq!(agent.cost.output_tokens, 30);
    assert!((agent.cost.cost_usd - 2.5).abs() < f64::EPSILON);
}

#[test]
fn apply_batch_folds_in_order_and_tracks_max_rev() {
    let mut view = MaterializedView::new();
    let batch = [
        entry(3, message("T1", 0x01)),
        entry(7, message("T1", 0x02)),
        entry(9, OpEntryKind::PhaseTransition { phase: Phase::Idle }),
    ];
    view.apply_batch("a1", &batch);

    let agent = view.get("a1").expect("agent present");
    assert_eq!(agent.rev, 9, "rev tracks the maximum folded");
    assert_eq!(
        agent.heads.threads.first().expect("T1").last_message_hash,
        ContentHash::new([0x02; 32]),
        "later message overwrites the earlier head",
    );
    assert_eq!(agent.phase, Some(Phase::Idle));
}

#[test]
fn durability_only_and_unknown_entries_are_inert() {
    let mut view = MaterializedView::new();
    view.apply(
        "a1",
        &entry(0, OpEntryKind::CommandEffect { cmd_id: "c".into(), dedup_token: "d".into() }),
    );
    view.apply("a1", &entry(1, OpEntryKind::SeenMark { dedup_token: "d".into() }));
    view.apply("a1", &entry(2, OpEntryKind::Unknown));

    let agent = view.get("a1").expect("agent present");
    assert_eq!(agent.rev, 2, "rev still advances");
    assert!(agent.heads.threads.is_empty());
    assert_eq!(agent.phase, None);
    assert_eq!(agent.lifecycle, None);
    assert_eq!(agent.cost, CostSnapshot::default());
}

#[test]
fn roster_create_archive_restore_cycle() {
    let mut view = MaterializedView::new();
    view.apply(
        "a1",
        &entry(
            0,
            OpEntryKind::ThreadCreated {
                thread_id: "T1".into(),
                name: "Refactor cache".into(),
                status: ThreadTurn::TheirTurn,
                timestamp_ms: 1_000,
            },
        ),
    );
    let agent = view.get("a1").expect("agent present");
    let e = agent.roster.first().expect("roster entry");
    assert_eq!(e.thread_id, "T1");
    assert_eq!(e.name, "Refactor cache");
    assert_eq!(e.status, ThreadTurn::TheirTurn);
    assert!(!e.archived);
    assert_eq!(e.last_activity_ms, 1_000);
    assert_eq!(e.msg_count, 0);

    view.apply("a1", &entry(1, OpEntryKind::ThreadArchived { thread_id: "T1".into() }));
    assert!(view.get("a1").expect("a").roster.first().expect("e").archived);

    view.apply("a1", &entry(2, OpEntryKind::ThreadRestored { thread_id: "T1".into() }));
    assert!(!view.get("a1").expect("a").roster.first().expect("e").archived);

    view.apply(
        "a1",
        &entry(
            3,
            OpEntryKind::ThreadStatusChanged {
                thread_id: "T1".into(),
                status: ThreadTurn::MyTurn,
            },
        ),
    );
    assert_eq!(
        view.get("a1").expect("a").roster.first().expect("e").status,
        ThreadTurn::MyTurn,
    );
}

#[test]
fn thread_created_folds_idempotently_on_replay() {
    let mut view = MaterializedView::new();
    let created = OpEntryKind::ThreadCreated {
        thread_id: "T1".into(),
        name: "Plan".into(),
        status: ThreadTurn::MyTurn,
        timestamp_ms: 5,
    };
    view.apply("a1", &entry(0, created.clone()));
    view.apply("a1", &entry(0, created)); // duplicate delivery / replay
    assert_eq!(
        view.get("a1").expect("agent").roster.len(),
        1,
        "a re-seen creation must refresh, never duplicate",
    );
}

#[test]
fn message_created_bumps_roster_count_and_activity() {
    let mut view = MaterializedView::new();
    view.apply(
        "a1",
        &entry(
            0,
            OpEntryKind::ThreadCreated {
                thread_id: "T1".into(),
                name: "Chat".into(),
                status: ThreadTurn::MyTurn,
                timestamp_ms: 100,
            },
        ),
    );
    // Two messages land; each bumps count + activity.
    let mut m1 = entry(1, message("T1", 0x01));
    m1.timestamp_ms = 200;
    let mut m2 = entry(2, message("T1", 0x02));
    m2.timestamp_ms = 350;
    view.apply("a1", &m1);
    view.apply("a1", &m2);

    let e = view.get("a1").expect("agent").roster.first().expect("entry");
    assert_eq!(e.msg_count, 2);
    assert_eq!(e.last_activity_ms, 350, "activity tracks the latest message");
}

#[test]
fn remove_drops_agent() {
    let mut view = MaterializedView::new();
    view.apply("a1", &entry(0, message("T1", 0x11)));
    assert_eq!(view.len(), 1);

    let removed = view.remove("a1");
    assert!(removed.is_some());
    assert!(view.is_empty());
    assert!(view.get("a1").is_none());
}
