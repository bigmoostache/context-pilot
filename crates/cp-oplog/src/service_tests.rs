//! Unit tests for [`crate::service`] — the off-loop group-commit thread and
//! its asymmetric backpressure policy (design doc GAP 2 / I2 / V11).
//!
//! Split out of `service.rs` to keep that file within the 500-line structure
//! limit; included via `#[path]` so `super` still resolves to `service`.

use super::*;
use crate::replay::replay;
use cp_wire::types::{ContentHash, Phase};
use tempfile::tempdir;

fn msg(thread: &str, byte: u8) -> OpEntryKind {
    OpEntryKind::MessageCreated {
        thread_id: thread.to_owned(),
        message_id: format!("m{byte}"),
        head: ContentHash::new([byte; 32]),
        inline_body: None,
    }
}

fn effect(token: &str) -> OpEntryKind {
    OpEntryKind::CommandEffect { cmd_id: token.to_owned(), dedup_token: token.to_owned() }
}

#[test]
fn durability_classification_matches_policy() {
    assert_eq!(Durability::of(&OpEntryKind::PhaseTransition { phase: Phase::Streaming }), Durability::BestEffort,);
    assert_eq!(
        Durability::of(&OpEntryKind::CostAggregate { input_tokens: 1, output_tokens: 2, cost_usd: 0.5 }),
        Durability::BestEffort,
    );
    assert_eq!(Durability::of(&effect("c1")), Durability::Durable);
    assert_eq!(Durability::of(&msg("T1", 1)), Durability::Durable);
    assert_eq!(Durability::of(&OpEntryKind::Unknown), Durability::Durable);
}

#[test]
fn durable_appends_are_monotonic_and_survive_reopen() {
    let dir = tempdir().expect("tempdir");
    {
        let service = Service::spawn(dir.path()).expect("spawn");
        let r0 = service.append_durable(effect("c0")).expect("c0");
        let r1 = service.append_durable(msg("T1", 0x11)).expect("m1");
        let r2 = service.append_durable(msg("T1", 0x22)).expect("m2");
        assert_eq!((r0, r1, r2), (0, 1, 2), "group commit assigns monotonic revs");
        service.shutdown().expect("shutdown");
    }
    // Reopen and replay: every durable record is present and folded.
    let state = replay(dir.path()).expect("replay");
    assert_eq!(state.rev_head, Some(2));
    assert!(state.seen.contains("c0"), "durable command effect survived");
    let mut expected_heads = cp_wire::types::snapshot::Heads::default();
    expected_heads.set_thread_head("T1", ContentHash::new([0x22; 32]));
    assert_eq!(state.heads, expected_heads, "latest head folded through the service");
}

#[test]
fn best_effort_is_accepted_when_unsaturated() {
    let dir = tempdir().expect("tempdir");
    let service = Service::spawn(dir.path()).expect("spawn");
    let outcome = service.append_best_effort(OpEntryKind::PhaseTransition { phase: Phase::Streaming });
    assert_eq!(outcome, BestEffortOutcome::Submitted);
    // A following durable append acts as a barrier: once it returns, the
    // phase record before it is durable too.
    let rev = service.append_durable(msg("T1", 0x33)).expect("durable barrier");
    assert!(rev >= 1, "phase + message each consumed a rev");
    service.shutdown().expect("shutdown");
}

#[test]
fn mixed_workload_replays_correctly() {
    let dir = tempdir().expect("tempdir");
    {
        let service = Service::spawn(dir.path()).expect("spawn");
        let _p = service.append_best_effort(OpEntryKind::PhaseTransition { phase: Phase::Tooling });
        let _a = service.append_durable(effect("cmd-a")).expect("a");
        let _m = service.append_durable(msg("T7", 0xAB)).expect("m");
        let _b = service.append_durable(effect("cmd-b")).expect("b");
        service.shutdown().expect("shutdown");
    }
    let state = replay(dir.path()).expect("replay");
    assert!(state.seen.contains("cmd-a"));
    assert!(state.seen.contains("cmd-b"));
    let mut expected = cp_wire::types::snapshot::Heads::default();
    expected.set_thread_head("T7", ContentHash::new([0xAB; 32]));
    assert_eq!(state.heads, expected);
}

#[test]
fn group_commit_rolls_segments_and_replays() {
    let dir = tempdir().expect("tempdir");
    {
        // Tiny limit forces rolls inside the commit thread; the buffered
        // roll must flush the old segment so nothing is lost.
        let service = Service::spawn_with_segment_limit(dir.path(), 16).expect("spawn");
        let mut last = 0;
        for byte in 0..12u8 {
            last = service.append_durable(msg("T1", byte)).expect("append");
        }
        assert!(last >= 11, "at least twelve user records were assigned revs");
        service.shutdown().expect("shutdown");
    }
    let state = replay(dir.path()).expect("replay");
    let mut expected = cp_wire::types::snapshot::Heads::default();
    expected.set_thread_head("T1", ContentHash::new([11; 32]));
    assert_eq!(state.heads, expected, "rolled, group-committed log replays intact");
}

#[test]
fn drop_without_shutdown_still_flushes() {
    let dir = tempdir().expect("tempdir");
    let last;
    {
        let service = Service::spawn(dir.path()).expect("spawn");
        last = service.append_durable(effect("only")).expect("append");
        // No explicit shutdown — Drop must join + the final batch is synced.
    }
    let state = replay(dir.path()).expect("replay");
    assert_eq!(state.rev_head, Some(last));
    assert!(state.seen.contains("only"));
}

#[test]
fn submit_durable_persists_without_awaiting_the_sync() {
    // The I2 main-loop path: submit_durable enqueues a durable record and
    // returns immediately (no ack wait). After a clean shutdown drains the
    // queue, the record must be durably present — proving "non-blocking"
    // did not mean "best-effort/droppable".
    let dir = tempdir().expect("tempdir");
    {
        let service = Service::spawn(dir.path()).expect("spawn");
        service.submit_durable(effect("fire-and-forget"));
        service.submit_durable(msg("T1", 0x44));
        // shutdown drains + flushes the in-flight batch before joining.
        service.shutdown().expect("shutdown");
    }
    let state = replay(dir.path()).expect("replay");
    assert!(state.seen.contains("fire-and-forget"), "detached-ack record is still durable");
    let mut expected = cp_wire::types::snapshot::Heads::default();
    expected.set_thread_head("T1", ContentHash::new([0x44; 32]));
    assert_eq!(state.heads, expected, "submitted head folded after off-loop commit");
}

#[test]
fn v11_emit_burst_never_blocks_the_loop_on_fsync() {
    // Design doc V11 / I2: the main loop NEVER fsyncs. A burst of phase
    // transitions during streaming must leave per-emit latency decoupled
    // from `fdatasync` latency — the loop only *enqueues*; the dedicated
    // commit thread group-commits off-loop.
    //
    // This is deterministic, not a flaky wall-clock race: `append_best_effort`
    // is a `try_send` on a bounded channel — it is *structurally*
    // non-blocking (returns `Dropped` instead of waiting when the queue is
    // full). So the strongest provable property is that no single emit call
    // stalls on the commit thread's `fdatasync`, regardless of disk speed.
    // We measure the WORST individual emit latency across a large burst and
    // assert it stays far below what even one fsync-per-call would cost.
    use std::time::Instant;

    let dir = tempdir().expect("tempdir");
    let service = Service::spawn(dir.path()).expect("spawn");

    const BURST: usize = 5_000;
    let mut worst = std::time::Duration::ZERO;
    let total = Instant::now();
    for i in 0..BURST {
        // Alternate Streaming/Tooling — the exact "phase transitions during
        // streaming" scenario V11 names.
        let phase = if i % 2 == 0 { Phase::Streaming } else { Phase::Tooling };
        let call = Instant::now();
        let _outcome = service.append_best_effort(OpEntryKind::PhaseTransition { phase });
        worst = worst.max(call.elapsed());
    }
    let burst_elapsed = total.elapsed();

    // No single emit may stall on a sync. A `try_send` is O(1); even a full
    // queue returns immediately. 25ms is orders of magnitude above the real
    // cost yet far below a single rotational fsync — a generous, non-flaky
    // ceiling that still fails loudly if an emit ever awaited durability.
    assert!(
        worst < std::time::Duration::from_millis(25),
        "worst single phase emit took {worst:?} — the loop blocked on something (V11/I2 violated)",
    );
    // The whole 5k burst must finish far faster than 5k isolated fsyncs
    // (seconds-to-minutes). 2s is a wide margin that still catches a
    // regression that put a sync on the emit path.
    assert!(
        burst_elapsed < std::time::Duration::from_secs(2),
        "5k phase emits took {burst_elapsed:?} — emit latency is coupled to fsync (V11/I2 violated)",
    );

    // Correctness is not sacrificed for the non-blocking emit: a durable
    // barrier still drains + group-commits the log intact and replayable.
    let rev = service.append_durable(effect("post-burst-barrier")).expect("barrier");
    service.shutdown().expect("shutdown");
    let state = replay(dir.path()).expect("replay");
    assert_eq!(state.rev_head, Some(rev), "the durable barrier is the latest rev");
    assert!(state.seen.contains("post-burst-barrier"), "the durable barrier survived the best-effort burst",);
}
