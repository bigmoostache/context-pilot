//! Phase 23 — `cp-oplog` dedup + compaction coverage at the public-API
//! boundary.
//!
//! The inline tests in `replay.rs` and `compact.rs` prove dedup-survives-roll
//! and replay-identical-after-compaction at point cases. This suite proves the
//! *end-to-end* exactly-once and reclamation properties the orchestration
//! design leans on, exercised only through the public surface (`OplogWriter`,
//! `replay`, `compact`, `segment`):
//!
//! * **V3 — a dedup token survives an arbitrarily long operational gap.** A
//!   command effect committed long ago (with many intervening records and
//!   segment rolls) is still recognised on replay, and a *duplicate* delivery
//!   of the same token folds as a no-op — the effect is applied exactly once,
//!   keeping the `rev` it first committed at (never time-evicted, design doc
//!   I4 / R2-1).
//! * **Compaction preserves replay byte-for-byte.** Over a rich multi-thread,
//!   multi-token, many-roll log, `replay` returns an identical [`Recovered`]
//!   (`rev_head` + heads + seen-set) before and after compaction, because the
//!   reclaimed segments are exactly the ones replay's fast path ignores and
//!   every live token rides the surviving checkpoint (the checkpoint *is* the
//!   ack barrier).
//! * **Compaction is safe and idempotent.** A second pass reclaims nothing, and
//!   a writer reopened after compaction keeps appending with correct replay.
//! * **The size trigger composes.** `total_bytes` feeding `over_threshold` is
//!   the real compaction trigger, and `body_gc_eligible` is parametric in its
//!   grace window (design doc GAP 3).
//!
//! [`Recovered`]: cp_oplog::replay::replay

use std::time::Duration;

use cp_oplog::append::OplogWriter;
use cp_oplog::compact::{Report, body_gc_eligible, compact, over_threshold, total_bytes};
use cp_oplog::replay::replay;
use cp_oplog::segment;
use cp_wire::types::ContentHash;
use cp_wire::types::oplog::OpEntryKind;

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

/// A tiny segment limit that forces a roll roughly every record.
const TINY_LIMIT: u64 = 16;

// ── V3: dedup across a long gap ──────────────────────────────────────────────

#[test]
fn a_dedup_token_survives_a_long_gap_and_a_duplicate_delivery_is_a_no_op() {
    let dir = tempfile::tempdir().expect("tempdir");

    let first_commit_rev;
    {
        let mut writer = OplogWriter::open_with_segment_limit(dir.path(), TINY_LIMIT).expect("open");

        // The original command effect commits early.
        first_commit_rev = writer.append(effect("tok-early")).expect("append effect");

        // A long operational gap: many intervening records force several rolls,
        // so the token now lives only inside a checkpoint snapshot, not in any
        // record near the tail.
        for byte in 0..24u8 {
            let _rev = writer.append(msg("T1", byte)).expect("append filler");
        }

        // A *duplicate* delivery of the same token arrives much later. It folds
        // idempotently: the running seen-set neither grows nor moves the rev.
        let dup_rev = writer.append(OpEntryKind::SeenMark { dedup_token: "tok-early".to_owned() }).expect("dup");
        assert!(dup_rev > first_commit_rev, "the duplicate is a later, distinct record");
        assert_eq!(
            writer.seen().rev_of("tok-early"),
            Some(first_commit_rev),
            "a duplicate delivery keeps the original commit rev (exactly-once)",
        );
    }

    // After a full reopen + replay, the token is still recognised at its first
    // commit rev — eviction is ack-driven, never time- or gap-driven.
    let recovered = replay(dir.path()).expect("replay");
    assert!(recovered.seen.contains("tok-early"), "token survives the gap + reopen");
    assert_eq!(recovered.seen.rev_of("tok-early"), Some(first_commit_rev), "replay preserves the original commit rev",);
    assert_eq!(recovered.seen.len(), 1, "the duplicate added no second entry");
}

// ── compaction preserves replay ─────────────────────────────────────────────

/// Build a rich, many-roll log: three threads, several command effects, all
/// interleaved under a tiny segment limit. Returns the oplog dir (kept alive by
/// the returned guard) and the tokens committed.
fn build_rich_log(guard: &tempfile::TempDir) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut writer = OplogWriter::open_with_segment_limit(guard.path(), TINY_LIMIT).expect("open");
    for step in 0..21u8 {
        let thread = match step % 3 {
            0 => "T1",
            1 => "T2",
            _ => "T3",
        };
        let _rev = writer.append(msg(thread, step)).expect("append msg");
        if step % 5 == 0 {
            let token = format!("tok-{step}");
            let _rev = writer.append(effect(&token)).expect("append effect");
            tokens.push(token);
        }
    }
    tokens
}

#[test]
fn replay_is_byte_for_byte_identical_across_compaction() {
    let dir = tempfile::tempdir().expect("tempdir");
    let tokens = build_rich_log(&dir);

    let before = replay(dir.path()).expect("replay before");
    let indices_before = segment::indices(dir.path()).expect("indices before");
    assert!(indices_before.len() > 2, "tiny limit must have produced several segments");

    let report = compact(dir.path()).expect("compact");
    assert!(report.segments_removed > 0, "stale segments must be reclaimed");

    let after = replay(dir.path()).expect("replay after");
    assert_eq!(before, after, "compaction must not change replay output at all");

    // Every committed token rode the surviving checkpoint.
    for token in &tokens {
        assert!(after.seen.contains(token), "live token {token} survives compaction");
    }

    // Only segments at or after the surviving checkpoint remain.
    let indices_after = segment::indices(dir.path()).expect("indices after");
    let oldest = report.oldest_index.expect("a surviving checkpoint segment");
    assert!(indices_after.iter().all(|&i| i >= oldest), "no segment older than the surviving checkpoint remains",);
}

#[test]
fn compaction_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _tokens = build_rich_log(&dir);

    let first = compact(dir.path()).expect("first compact");
    assert!(first.segments_removed > 0, "first pass reclaims stale segments");

    let second = compact(dir.path()).expect("second compact");
    assert_eq!(second.segments_removed, 0, "a second pass has nothing left to reclaim");
    assert_eq!(second.oldest_index, first.oldest_index, "the surviving boundary is stable");
}

#[test]
fn a_writer_reopened_after_compaction_keeps_appending_correctly() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _tokens = build_rich_log(&dir);
    let _report = compact(dir.path()).expect("compact");

    let before = replay(dir.path()).expect("replay after compact");

    // Reopen and append a fresh head: replay must reflect it on top of the
    // compacted base, with a strictly larger rev.
    let new_rev;
    {
        let mut writer = OplogWriter::open_with_segment_limit(dir.path(), TINY_LIMIT).expect("reopen");
        new_rev = writer.append(msg("T1", 250)).expect("append after compact");
    }
    let after = replay(dir.path()).expect("replay after new append");

    assert!(after.rev_head > before.rev_head, "the post-compaction append advances rev_head past the compacted base",);
    assert_eq!(after.rev_head, Some(new_rev), "replay sees the freshly appended record");
    let t1 = after.heads.threads.iter().find(|h| h.thread_id == "T1").expect("T1 head present");
    assert_eq!(t1.last_message_hash, ContentHash::new([250; 32]), "T1 head is the fresh write");
}

#[test]
fn compaction_is_a_noop_before_the_first_roll() {
    let dir = tempfile::tempdir().expect("tempdir");
    {
        // Default limit: a few small records never roll, so seg-0 stands alone
        // with no leading checkpoint — nothing to reclaim.
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        for byte in 0..4u8 {
            let _rev = writer.append(msg("T1", byte)).expect("append");
        }
    }
    let report = compact(dir.path()).expect("compact");
    assert_eq!(report, Report::new(0, Some(0)));
}

// ── size trigger + GC grace composition ─────────────────────────────────────

#[test]
fn total_bytes_feeds_the_over_threshold_compaction_trigger() {
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let mut writer = OplogWriter::open_with_segment_limit(dir.path(), TINY_LIMIT).expect("open");
        for byte in 0..12u8 {
            let _rev = writer.append(msg("T1", byte)).expect("append");
        }
    }
    let bytes = total_bytes(dir.path()).expect("total_bytes");
    assert!(bytes > 0, "a non-empty log has positive byte size");

    // A threshold below the current size trips the trigger; one above does not.
    assert!(over_threshold(bytes, bytes.saturating_sub(1)), "size over a smaller threshold trips");
    assert!(!over_threshold(bytes, bytes), "size equal to the threshold does not trip");
    assert!(!over_threshold(bytes, bytes.saturating_add(1)), "size under a larger threshold is fine");
}

#[test]
fn body_gc_eligibility_is_parametric_in_its_grace_window() {
    // The grace window is a knob, not a hardcoded constant: a body is eligible
    // only once it is strictly older than the supplied grace.
    let grace = Duration::from_millis(10);
    assert!(!body_gc_eligible(Duration::from_millis(5), grace), "younger than grace: keep");
    assert!(!body_gc_eligible(grace, grace), "exactly at grace: keep (strict greater-than)");
    assert!(body_gc_eligible(Duration::from_millis(20), grace), "older than grace: eligible");
}
