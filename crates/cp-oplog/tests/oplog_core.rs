//! Phase 22 — `cp-oplog` append / segment / replay coverage at the public-API
//! boundary.
//!
//! The inline unit tests in `append.rs`, `segment.rs`, and `replay.rs` prove
//! each mechanism at point cases (a handful of appends, one hand-flipped byte,
//! a single reopen). This suite proves the *properties* those mechanisms
//! guarantee, exercised only through the crate's public surface
//! (`OplogWriter`, `segment::{path, indices, read}`, `replay`) — no access to
//! the crate-internal `replay_full` / `fold_entry`, so every assertion is a
//! genuine black-box property the rest of the system depends on:
//!
//! * **`rev` is strictly monotonic and never reused** across many segment rolls
//!   and several reopens;
//! * **every rolled segment opens with a leading checkpoint** (and `seg-0` does
//!   not) — the bounded-replay invariant (design doc GAP 1 / I5);
//! * **`replay` rebuilds exactly the ground-truth heads + seen-set** tracked
//!   independently by the test, over a multi-thread, multi-segment, reopened
//!   log (the fast checkpoint-seeded path, validated against truth);
//! * **torn-tail recovery is correct at every byte offset** — reopening a
//!   segment truncated to any length recovers precisely the records whose
//!   frames survived intact, never a partial or fabricated one (design doc V1).

// This integration target links `cp-oplog`'s deps; both are used directly
// below, but the per-target `unused-crate-dependencies` lint is satisfied only
// by a direct path reference, so name them explicitly via the imports rather
// than `as _`.
use std::fs;

use cp_oplog::append::OplogWriter;
use cp_oplog::replay::replay;
use cp_oplog::segment;
use cp_wire::types::ContentHash;
use cp_wire::types::oplog::OpEntryKind;

// ── helpers ────────────────────────────────────────────────────────────────

/// A `MessageCreated` for `thread`, tagged with `byte` so the head hash is
/// distinguishable per write.
fn msg(thread: &str, byte: u8) -> OpEntryKind {
    OpEntryKind::MessageCreated {
        thread_id: thread.to_owned(),
        message_id: format!("m{byte}"),
        head: ContentHash::new([byte; 32]),
        inline_body: None,
    }
}

/// A tiny segment limit that forces a roll roughly every record, so multi-
/// segment behaviour is exercised without writing megabytes.
const TINY_LIMIT: u64 = 16;

// ── rev monotonicity ────────────────────────────────────────────────────────

#[test]
fn revs_are_strictly_monotonic_across_rolls_and_reopens() {
    let dir = tempfile::tempdir().expect("tempdir");

    // First session: many appends under a tiny limit → many rolls.
    let mut all_revs = Vec::new();
    {
        let mut writer = OplogWriter::open_with_segment_limit(dir.path(), TINY_LIMIT).expect("open");
        for byte in 0..40u8 {
            all_revs.push(writer.append(msg("T1", byte)).expect("append"));
        }
    }

    // Second session: reopen and append more; revs must continue past the last.
    {
        let mut writer = OplogWriter::open_with_segment_limit(dir.path(), TINY_LIMIT).expect("reopen");
        for byte in 40..60u8 {
            all_revs.push(writer.append(msg("T1", byte)).expect("append"));
        }
    }

    // Every announced rev is strictly greater than its predecessor — the
    // user-visible revs (checkpoints consume revs too, so gaps are allowed, but
    // strict increase and no reuse are not).
    for pair in all_revs.windows(2) {
        let (prev, next) = (pair.first().expect("prev"), pair.get(1).expect("next"));
        assert!(next > prev, "rev must strictly increase: {prev} then {next}");
    }

    // No rev is reused anywhere across both sessions.
    let mut sorted = all_revs.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), all_revs.len(), "a rev was reused across reopen");
}

#[test]
fn reopen_resumes_at_next_rev_without_reuse() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stop_rev;
    {
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        let mut last = 0;
        for byte in 0..5u8 {
            last = writer.append(msg("T1", byte)).expect("append");
        }
        stop_rev = last;
    }
    let mut writer = OplogWriter::open(dir.path()).expect("reopen");
    assert_eq!(writer.next_rev(), stop_rev.wrapping_add(1), "resume past highest durable rev");
    let resumed = writer.append(msg("T1", 99)).expect("append");
    assert_eq!(resumed, stop_rev.wrapping_add(1), "first post-reopen rev is exactly next_rev");
}

// ── leading-checkpoint invariant (GAP 1 / I5) ───────────────────────────────

#[test]
fn every_rolled_segment_opens_with_a_checkpoint_and_seg0_does_not() {
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let mut writer = OplogWriter::open_with_segment_limit(dir.path(), TINY_LIMIT).expect("open");
        for byte in 0..30u8 {
            let _rev = writer.append(msg("T1", byte)).expect("append");
        }
    }

    let indices = segment::indices(dir.path()).expect("indices");
    assert!(indices.len() > 2, "tiny limit must have rolled several segments");

    for &index in &indices {
        let scan = segment::read(&segment::path(dir.path(), index)).expect("read");
        let first = scan.entries.first().expect("a rolled/seeded segment is never empty");
        let opens_with_checkpoint = matches!(first.kind, OpEntryKind::Checkpoint { .. });
        if index == 0 {
            assert!(!opens_with_checkpoint, "seg-0 has no prior state, so it must NOT open with a checkpoint",);
        } else {
            assert!(opens_with_checkpoint, "rolled segment {index} must open with a leading checkpoint",);
        }
    }
}

#[test]
fn segment_indices_are_ascending_and_contiguous_from_zero() {
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let mut writer = OplogWriter::open_with_segment_limit(dir.path(), TINY_LIMIT).expect("open");
        for byte in 0..24u8 {
            let _rev = writer.append(msg("T1", byte)).expect("append");
        }
    }
    let indices = segment::indices(dir.path()).expect("indices");
    let expected: Vec<u64> = (0..u64::try_from(indices.len()).unwrap_or(0)).collect();
    assert_eq!(indices, expected, "segment indices must be 0,1,2,… in order");
}

// ── replay rebuilds ground truth ────────────────────────────────────────────

#[test]
fn replay_rebuilds_ground_truth_heads_and_seen_over_a_rolled_reopened_log() {
    let dir = tempfile::tempdir().expect("tempdir");

    // Ground truth tracked alongside the writes: last head byte per thread, and
    // the set of dedup tokens seen.
    let mut expected_last: std::collections::BTreeMap<String, u8> = std::collections::BTreeMap::new();
    let mut expected_tokens: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    {
        let mut writer = OplogWriter::open_with_segment_limit(dir.path(), TINY_LIMIT).expect("open");
        // Interleave three threads' messages with command effects, forcing many
        // rolls so replay must lean on the checkpoint fast path.
        for step in 0..18u8 {
            let thread = match step % 3 {
                0 => "T1",
                1 => "T2",
                _ => "T3",
            };
            let _rev = writer.append(msg(thread, step)).expect("append");
            let _prev = expected_last.insert(thread.to_owned(), step);

            if step % 4 == 0 {
                let token = format!("tok-{step}");
                let _rev = writer
                    .append(OpEntryKind::CommandEffect { cmd_id: format!("c{step}"), dedup_token: token.clone() })
                    .expect("append effect");
                let _new = expected_tokens.insert(token);
            }
        }
    }

    // Reopen and add one more head per thread, so replay must also cross the
    // session boundary correctly.
    {
        let mut writer = OplogWriter::open_with_segment_limit(dir.path(), TINY_LIMIT).expect("reopen");
        for (thread, byte) in [("T1", 200u8), ("T2", 201u8), ("T3", 202u8)] {
            let _rev = writer.append(msg(thread, byte)).expect("append");
            let _prev = expected_last.insert(thread.to_owned(), byte);
        }
    }

    let recovered = replay(dir.path()).expect("replay");

    // Every thread's recovered head must be the last byte we wrote for it.
    for (thread, byte) in &expected_last {
        let head = recovered
            .heads
            .threads
            .iter()
            .find(|h| &h.thread_id == thread)
            .unwrap_or_else(|| panic!("thread {thread} missing from recovered heads"));
        assert_eq!(
            head.last_message_hash,
            ContentHash::new([*byte; 32]),
            "thread {thread} head must be its last write",
        );
    }
    assert_eq!(recovered.heads.threads.len(), expected_last.len(), "no phantom threads in recovered heads",);

    // Every committed dedup token must be in the recovered seen-set, and none
    // beyond them.
    for token in &expected_tokens {
        assert!(recovered.seen.contains(token), "token {token} must survive replay");
    }
    assert_eq!(recovered.seen.len(), expected_tokens.len(), "recovered seen-set holds exactly the committed tokens",);
}

// ── torn-tail recovery at every byte offset (V1) ─────────────────────────────

/// Build a single-segment oplog of `n` message records and return the raw
/// segment bytes plus, for each appended rev, the segment file's byte length
/// immediately after that append (a clean record-end boundary).
fn build_single_segment(n: u8) -> (Vec<u8>, Vec<(u64, u64)>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut boundaries = Vec::new();
    {
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        for byte in 0..n {
            let rev = writer.append(msg("T1", byte)).expect("append");
            let len = fs::metadata(segment::path(dir.path(), 0)).expect("stat").len();
            boundaries.push((rev, len));
        }
    }
    let bytes = fs::read(segment::path(dir.path(), 0)).expect("read seg0");
    (bytes, boundaries)
}

/// The highest rev whose full frame ends at or before `cut`, or `None`.
fn expected_rev_head(boundaries: &[(u64, u64)], cut: u64) -> Option<u64> {
    boundaries.iter().filter(|(_rev, end)| *end <= cut).map(|(rev, _end)| *rev).next_back()
}

#[test]
fn torn_tail_recovery_is_correct_at_every_byte_offset() {
    let (full_bytes, boundaries) = build_single_segment(6);

    for cut in 0..=full_bytes.len() {
        // A fresh oplog dir containing only the truncated segment.
        let dir = tempfile::tempdir().expect("tempdir");
        let prefix = full_bytes.get(..cut).expect("cut in range");
        fs::write(segment::path(dir.path(), 0), prefix).expect("write truncated seg");

        // Reopen must never error: a torn tail is truncated, not fatal.
        let writer = OplogWriter::open(dir.path()).expect("reopen truncated oplog");
        let recovered = replay(dir.path()).expect("replay truncated oplog");

        let cut_u64 = u64::try_from(cut).unwrap_or(u64::MAX);
        let want = expected_rev_head(&boundaries, cut_u64);
        assert_eq!(recovered.rev_head, want, "cut {cut}: recovered rev_head must be the last intact record",);

        // next_rev resumes one past the recovered head (or 0 from empty).
        let want_next = want.map_or(0, |rev| rev.wrapping_add(1));
        assert_eq!(writer.next_rev(), want_next, "cut {cut}: next_rev resumes without reuse");
    }
}

#[test]
fn recovery_resumes_appending_without_reuse_after_a_mid_frame_truncation() {
    let (full_bytes, boundaries) = build_single_segment(4);
    // Truncate one byte into the final frame: the last record is torn away.
    let (last_rev, last_end) = *boundaries.last().expect("at least one record");
    let (_prev_rev, prev_end) = *boundaries.get(boundaries.len().wrapping_sub(2)).expect("two records");
    let cut = usize::try_from(prev_end.wrapping_add(1)).unwrap_or(0);
    assert!(cut < usize::try_from(last_end).unwrap_or(usize::MAX), "cut lands inside the last frame");

    let dir = tempfile::tempdir().expect("tempdir");
    let prefix = full_bytes.get(..cut).expect("cut in range");
    fs::write(segment::path(dir.path(), 0), prefix).expect("write truncated seg");

    let mut writer = OplogWriter::open(dir.path()).expect("reopen");
    // The torn final record is gone; the next append reclaims its rev slot.
    let resumed = writer.append(msg("T1", 250)).expect("append after recovery");
    assert_eq!(resumed, last_rev, "the torn record's rev is reissued to the fresh append");

    let recovered = replay(dir.path()).expect("replay");
    assert_eq!(recovered.rev_head, Some(last_rev), "replay sees the fresh record at the recovered slot");
}
