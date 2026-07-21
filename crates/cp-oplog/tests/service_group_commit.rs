//! Phase 24 — `cp-oplog` group-commit service coverage at the public-API
//! boundary.
//!
//! The inline tests in `service.rs` prove the happy paths (monotonic durable
//! revs, best-effort accepted when idle, a mixed workload replays, Drop
//! flushes). This suite proves the properties that only emerge under
//! *concurrency* and *pressure* — the reasons the off-loop group-commit thread
//! and its asymmetric backpressure exist (design doc GAP 2 / I2):
//!
//! * **`Durability::of` classifies every `OpEntryKind`** — a single source of
//!   truth for which records may drop and which must block, exhaustive over all
//!   eight discriminants so a future variant can't slip through unclassified.
//! * **Concurrent durable submitters get unique, contiguous revs.** Many
//!   threads hammering one service receive `rev`s `0..N` with no gap, dup, or
//!   reorder — proof the single commit thread is the sole, serialised
//!   rev-assigner even under contention.
//! * **Best-effort never blocks the submitter and sheds load under flood.** A
//!   flood far larger than the queue completes promptly and drops the overflow
//!   (the GAP 2 asymmetry: disposable records yield rather than stall), while
//!   the service stays healthy for the durable records that follow.
//! * **The final batch is flushed on shutdown**, and an interleaved durable /
//!   best-effort workload loses no durable record.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cp_oplog::replay::replay;
use cp_oplog::service::{BestEffortOutcome, Durability, Service};
use cp_wire::types::oplog::OpEntryKind;
use cp_wire::types::snapshot::Snapshot;
use cp_wire::types::{ContentHash, LifecycleState, Phase};

// ── helpers ────────────────────────────────────────────────────────────────

/// A `MessageCreated` for `thread`, tagged with `byte`.
fn msg(thread: &str, byte: u8) -> OpEntryKind {
    OpEntryKind::MessageCreated {
        thread_id: thread.to_owned(),
        message_id: format!("m{byte}"),
        head: ContentHash::new([byte; 32]),
        inline_body: None,
    }
}

/// A `CommandEffect` carrying `token` as its dedup key.
fn effect(token: &str) -> OpEntryKind {
    OpEntryKind::CommandEffect { cmd_id: format!("cmd-{token}"), dedup_token: token.to_owned() }
}

/// A disposable best-effort phase record.
fn phase() -> OpEntryKind {
    OpEntryKind::PhaseTransition { phase: Phase::Streaming }
}

// ── classification ──────────────────────────────────────────────────────────

#[test]
fn durability_of_is_exhaustive_over_every_kind() {
    // Best-effort: only the two disposable, self-healing classes.
    assert_eq!(Durability::of(&phase()), Durability::BestEffort);
    assert_eq!(
        Durability::of(&OpEntryKind::CostAggregate { input_tokens: 1, output_tokens: 2, cost_usd: 0.5 }),
        Durability::BestEffort,
    );

    // Durable: everything effect-bearing, plus the conservative Unknown.
    assert_eq!(Durability::of(&effect("c")), Durability::Durable);
    assert_eq!(Durability::of(&OpEntryKind::SeenMark { dedup_token: "d".to_owned() }), Durability::Durable,);
    assert_eq!(Durability::of(&msg("T1", 1)), Durability::Durable);
    assert_eq!(Durability::of(&OpEntryKind::Lifecycle { state: LifecycleState::Running }), Durability::Durable,);
    assert_eq!(Durability::of(&OpEntryKind::Checkpoint { snapshot: Snapshot::default() }), Durability::Durable,);
    assert_eq!(
        Durability::of(&OpEntryKind::Unknown),
        Durability::Durable,
        "an unrecognised future record must never be silently dropped",
    );
}

// ── concurrency ─────────────────────────────────────────────────────────────

#[test]
fn concurrent_durable_submitters_get_unique_contiguous_revs() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Default segment limit: 400 tiny records never roll, so no checkpoint
    // consumes a rev — the durable revs are exactly 0..400, contiguous.
    let service = Arc::new(Service::spawn(dir.path()).expect("spawn"));

    let n_threads = 8u8;
    let per_thread = 50u8;
    let mut handles = Vec::new();
    for t in 0..n_threads {
        let svc = Arc::clone(&service);
        handles.push(thread::spawn(move || {
            let mut revs = Vec::new();
            for i in 0..per_thread {
                let token = format!("t{t}-{i}");
                revs.push(svc.append_durable(effect(&token)).expect("durable append"));
            }
            revs
        }));
    }

    let mut all_revs = Vec::new();
    for handle in handles {
        all_revs.extend(handle.join().expect("thread joined"));
    }

    // Drop the last Arc → Service::drop joins the commit thread and syncs the
    // final batch, so the log is durable before we replay.
    drop(service);

    let total = usize::from(n_threads).saturating_mul(usize::from(per_thread));
    assert_eq!(all_revs.len(), total, "every durable append returned a rev");

    let mut sorted = all_revs.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), total, "no rev was reused across threads");
    let expected: Vec<u64> = (0..u64::try_from(total).unwrap_or(0)).collect();
    assert_eq!(sorted, expected, "durable revs are exactly 0..N, no gaps under contention");

    // Replay confirms the durable truth: every token landed exactly once.
    let recovered = replay(dir.path()).expect("replay");
    assert_eq!(recovered.rev_head, Some(u64::try_from(total.saturating_sub(1)).unwrap_or(0)));
    assert_eq!(recovered.seen.len(), total, "every command effect is in the seen-set");
}

// ── backpressure asymmetry ──────────────────────────────────────────────────

#[test]
fn best_effort_floods_drop_without_ever_blocking_the_submitter() {
    let dir = tempfile::tempdir().expect("tempdir");
    let service = Service::spawn(dir.path()).expect("spawn");

    // A flood orders of magnitude beyond the queue capacity. The producer must
    // never block (try_send fails fast), so it completes promptly, and the
    // overflow — vastly more than any single fdatasync batch can absorb — must
    // produce drops (the GAP 2 best-effort shedding policy).
    let flood = 200_000u64;
    let start = Instant::now();
    let mut dropped = 0u64;
    let mut submitted = 0u64;
    for _unused in 0..flood {
        if matches!(service.append_best_effort(phase()), BestEffortOutcome::Submitted) {
            submitted = submitted.wrapping_add(1);
        } else {
            dropped = dropped.wrapping_add(1);
        }
    }
    let elapsed = start.elapsed();

    assert!(elapsed < Duration::from_secs(5), "best-effort submit never blocks the producer");
    assert_eq!(submitted.wrapping_add(dropped), flood, "every submission was accounted for");
    assert!(dropped > 0, "a flood far beyond the queue must shed load");
    assert!(submitted > 0, "some best-effort records still made it through");

    // The service is unharmed: a durable record after the flood still commits
    // and replays. (best-effort phases leave no trace in heads/seen, so we
    // assert via the durable record only.)
    let rev = service.append_durable(effect("after-flood")).expect("durable after flood");
    service.shutdown().expect("shutdown");

    let recovered = replay(dir.path()).expect("replay");
    assert!(recovered.seen.contains("after-flood"), "the post-flood durable record survives");
    assert!(
        recovered.rev_head >= Some(rev),
        "rev_head is at least the post-flood durable rev (best-effort writes may precede it)",
    );
}

// ── shutdown flushing ───────────────────────────────────────────────────────

#[test]
fn a_large_final_batch_is_flushed_on_shutdown() {
    let dir = tempfile::tempdir().expect("tempdir");
    let service = Service::spawn(dir.path()).expect("spawn");

    // Submit a run of durable records; the last few may still be batching when
    // shutdown is requested — shutdown must drain and sync them.
    let mut last = 0;
    for byte in 0..30u8 {
        last = service.append_durable(msg("T1", byte)).expect("durable");
    }
    service.shutdown().expect("shutdown flushes the final batch");

    let recovered = replay(dir.path()).expect("replay");
    assert_eq!(recovered.rev_head, Some(last), "the last durable rev survived shutdown");
    let t1 = recovered.heads.threads.iter().find(|h| h.thread_id == "T1").expect("T1 head present");
    assert_eq!(t1.last_message_hash, ContentHash::new([29; 32]), "the final head is durable");
}

#[test]
fn interleaved_durable_and_best_effort_preserves_every_durable_record() {
    let dir = tempfile::tempdir().expect("tempdir");
    let service = Service::spawn(dir.path()).expect("spawn");

    let mut tokens = Vec::new();
    for step in 0..20u8 {
        // A disposable phase between durable records — it must not perturb the
        // durable stream's exactly-once guarantee.
        let _ignored = service.append_best_effort(phase());
        let token = format!("tok-{step}");
        let _rev = service.append_durable(effect(&token)).expect("durable");
        tokens.push(token);
        let _head = service.append_durable(msg("T1", step)).expect("durable head");
    }
    service.shutdown().expect("shutdown");

    let recovered = replay(dir.path()).expect("replay");
    for token in &tokens {
        assert!(recovered.seen.contains(token), "durable token {token} survived the interleave");
    }
    assert_eq!(recovered.seen.len(), tokens.len(), "no durable effect lost or duplicated");
    let t1 = recovered.heads.threads.iter().find(|h| h.thread_id == "T1").expect("T1 head present");
    assert_eq!(t1.last_message_hash, ContentHash::new([19; 32]), "last durable head is correct");
}
