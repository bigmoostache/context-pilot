//! Compaction — reclaim oplog disk while preserving replay semantics, and the
//! orphan-body GC grace rule (design doc GAP 3).
//!
//! # Segment compaction
//!
//! The oplog grows one segment at a time and never rewrites a record, so
//! without compaction the log is unbounded. Compaction exploits the
//! leading-checkpoint invariant (design doc GAP 1, [`crate::append`]): every
//! rolled segment opens with a full [`Snapshot`] checkpoint, and replay's fast
//! path seeds from the **newest** checkpoint-bearing segment and reads only it
//! ([`crate::replay`]). Therefore every segment *older* than that one is
//! redundant for replay — its heads are subsumed by the checkpoint, and every
//! still-live dedup token is carried verbatim in the checkpoint's
//! [`SeenSet`](cp_wire::types::snapshot::SeenSet). [`compact`] simply deletes
//! those older segments.
//!
//! This is why compaction needs no separate "ack-rev barrier" pass: a token
//! retires from the running seen-set (via
//! [`SeenSet::evict_through`](cp_wire::types::snapshot::SeenSet::evict_through))
//! *before* a later checkpoint is stamped, so an acknowledged token is already
//! absent from the checkpoint, while an un-acknowledged one is still present
//! and thus survives compaction. The checkpoint *is* the barrier.
//!
//! Replay-after-compaction is byte-for-byte identical to replay-before, because
//! the segments removed are exactly the ones replay's fast path already
//! ignored.
//!
//! # GAP 3 — orphan-body GC grace rule
//!
//! Large message bodies spill to `oplog/bodies/{hash}` and are `fdatasync`'d
//! **before** the oplog entry that references them commits (the I13
//! body-before-reference barrier). A body in that directory with no referencing
//! entry is therefore one of two things, indistinguishable by reference alone:
//!
//! 1. a **crash-orphan** — the body became durable but the referencing entry
//!    never committed (a crash struck inside the barrier window); safe to
//!    delete; or
//! 2. an **in-flight spill** — the body became durable microseconds ago and its
//!    referencing entry is about to commit *right now*; deleting it would break
//!    the barrier and dangle a head.
//!
//! [`body_gc_eligible`] resolves the race with an **age grace window**: a body
//! is GC-eligible only once it is older than [`DEFAULT_GC_GRACE`], which is
//! orders of magnitude larger than the longest possible barrier window (a spill
//! plus a single append + `fdatasync` on one writer thread). Any in-flight
//! spill is referenced long before it ages past the grace, so only true
//! crash-orphans are ever collected. The body-store sweep that calls this lands
//! with the body store itself (a later phase); the decision rule is defined and
//! tested here, where compaction's contract lives.

use std::fs;
use std::path::Path;
use std::time::Duration;

use cp_wire::types::oplog::OpEntryKind;

use crate::error::OplogResult;
use crate::segment;

/// Default size threshold above which [`should_compact`] advises a compaction
/// pass: 256 MiB of segments (four default 64 MiB segments).
pub const DEFAULT_COMPACT_THRESHOLD: u64 = 256 * 1024 * 1024;

/// Default grace window for orphan-body GC (design doc GAP 3).
///
/// Must exceed the longest possible I13 barrier window — the time between a
/// body becoming durable (`fdatasync`) and its referencing entry committing,
/// which is a handful of `fdatasync`s on a single writer thread. 60 s is vastly
/// larger, so an in-flight spilled body is always referenced before it becomes
/// GC-eligible; only crash-orphans are ever collected.
pub const DEFAULT_GC_GRACE: Duration = Duration::from_mins(1);

/// The outcome of a [`compact`] pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// How many whole segments were deleted.
    pub segments_removed: u64,

    /// The index of the oldest segment remaining after compaction, or `None`
    /// if the oplog directory holds no segments.
    pub oldest_index: Option<u64>,
}

impl Report {
    /// Bundle a compaction outcome. A constructor keeps [`Report`]
    /// `#[non_exhaustive]` across cross-crate builders (integration tests
    /// assert against a constructed expected value).
    #[must_use]
    pub const fn new(segments_removed: u64, oldest_index: Option<u64>) -> Self {
        Self { segments_removed, oldest_index }
    }
}

/// Compact the oplog in `dir`: delete every segment older than the newest
/// checkpoint-bearing segment.
///
/// This preserves replay output exactly (see the module docs): the deleted
/// segments are precisely the ones replay's fast path ignores. When no segment
/// opens with a checkpoint (a young, never-rolled log), it is a no-op.
///
/// Safe to run against a live writer: the writer only ever appends to the
/// newest segment, which compaction never removes.
///
/// # Errors
///
/// Returns [`Error::Io`](crate::error::Error::Io) if a segment
/// cannot be listed, read, removed, or if the directory cannot be `fsync`'d.
pub fn compact<P>(path: P) -> OplogResult<Report>
where
    P: AsRef<Path>,
{
    let dir = path.as_ref();
    let indices = segment::indices(dir)?;

    let Some(cut) = newest_checkpoint_index(dir, &indices)? else {
        return Ok(Report { segments_removed: 0, oldest_index: indices.first().copied() });
    };

    let mut removed: u64 = 0;
    for &index in &indices {
        if index < cut {
            fs::remove_file(segment::path(dir, index))?;
            removed = removed.wrapping_add(1);
        }
    }
    if removed > 0 {
        sync_dir(dir)?;
    }
    Ok(Report { segments_removed: removed, oldest_index: Some(cut) })
}

/// The highest segment index whose first record is a checkpoint — the segment
/// replay seeds from, and the boundary below which everything is redundant.
/// `None` when no segment opens with a checkpoint.
fn newest_checkpoint_index(dir: &Path, indices: &[u64]) -> OplogResult<Option<u64>> {
    for &index in indices.iter().rev() {
        let scan = segment::read(&segment::path(dir, index))?;
        if let Some(first) = scan.entries.first()
            && matches!(first.kind, OpEntryKind::Checkpoint { .. })
        {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

/// `fsync` the directory so removed segments' directory entries are durably
/// gone (a crash after compaction never resurrects a deleted segment).
fn sync_dir(dir: &Path) -> OplogResult<()> {
    let handle = fs::File::open(dir)?;
    handle.sync_all()?;
    Ok(())
}

/// The total byte size of all segments in `dir`, for a size-driven compaction
/// trigger.
///
/// # Errors
///
/// Returns [`Error::Io`](crate::error::Error::Io) if the directory
/// cannot be listed or a segment's metadata cannot be read.
pub fn total_bytes<P>(path: P) -> OplogResult<u64>
where
    P: AsRef<Path>,
{
    let dir = path.as_ref();
    let mut total: u64 = 0;
    for index in segment::indices(dir)? {
        let meta = fs::metadata(segment::path(dir, index))?;
        total = total.wrapping_add(meta.len());
    }
    Ok(total)
}

/// Whether the oplog is large enough to warrant compaction.
#[must_use]
pub const fn over_threshold(total_bytes: u64, threshold: u64) -> bool {
    total_bytes > threshold
}

/// Whether an unreferenced spilled body is eligible for garbage collection
/// (design doc GAP 3).
///
/// Returns `true` only when `body_age` exceeds `grace` — long enough that any
/// in-flight spill (whose referencing entry commits within the I13 barrier
/// window) has already been referenced, so only crash-orphans qualify. Use
/// [`DEFAULT_GC_GRACE`] for `grace` unless a measured barrier window justifies
/// otherwise.
#[must_use]
pub const fn body_gc_eligible(body_age: Duration, grace: Duration) -> bool {
    body_age.as_nanos() > grace.as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::append::OplogWriter;
    use crate::replay::replay;
    use cp_wire::types::ContentHash;
    use tempfile::tempdir;

    fn msg(thread: &str, byte: u8) -> OpEntryKind {
        OpEntryKind::MessageCreated {
            thread_id: thread.to_owned(),
            message_id: format!("m{byte}"),
            head: ContentHash::new([byte; 32]),
            inline_body: None,
        }
    }

    #[test]
    fn compact_removes_pre_checkpoint_segments() {
        let dir = tempdir().expect("tempdir");
        {
            // A tiny limit forces many rolls, so several checkpoint-bearing
            // segments accumulate.
            let mut writer = OplogWriter::open_with_segment_limit(dir.path(), 16).expect("open");
            for byte in 0..10u8 {
                let _r = writer.append(msg("T1", byte)).expect("append");
            }
        }
        let before = segment::indices(dir.path()).expect("indices");
        assert!(before.len() > 1, "tiny limit must have rolled");

        let report = compact(dir.path()).expect("compact");
        assert!(report.segments_removed > 0, "old segments must be reclaimed");

        let after = segment::indices(dir.path()).expect("indices");
        let newest = *before.last().expect("newest");
        assert_eq!(after, vec![newest], "only the newest checkpoint segment remains");
        assert_eq!(report.oldest_index, Some(newest));
    }

    #[test]
    fn replay_is_identical_after_compaction() {
        let dir = tempdir().expect("tempdir");
        {
            let mut writer = OplogWriter::open_with_segment_limit(dir.path(), 16).expect("open");
            let _c = writer
                .append(OpEntryKind::CommandEffect { cmd_id: "c1".to_owned(), dedup_token: "tok-keep".to_owned() })
                .expect("append");
            for byte in 0..10u8 {
                let _r = writer.append(msg("T1", byte)).expect("append");
            }
            let _last = writer.append(msg("T2", 0xEE)).expect("append");
        }
        let before = replay(dir.path()).expect("replay before");
        let report = compact(dir.path()).expect("compact");
        assert!(report.segments_removed > 0);
        let after = replay(dir.path()).expect("replay after");

        assert_eq!(before, after, "compaction must not change replay output");
        // The un-acknowledged token rode the checkpoint and survives.
        assert!(after.seen.contains("tok-keep"), "live dedup token survives compaction");
    }

    #[test]
    fn compact_is_noop_on_single_segment() {
        let dir = tempdir().expect("tempdir");
        {
            let mut writer = OplogWriter::open(dir.path()).expect("open");
            for byte in 0..4u8 {
                let _r = writer.append(msg("T1", byte)).expect("append");
            }
        }
        // seg-0 carries no leading checkpoint, so there is nothing to compact.
        let report = compact(dir.path()).expect("compact");
        assert_eq!(report.segments_removed, 0);
        assert_eq!(report.oldest_index, Some(0));
        assert_eq!(segment::indices(dir.path()).expect("indices"), vec![0]);
    }

    #[test]
    fn compact_on_empty_dir_is_noop() {
        let dir = tempdir().expect("tempdir");
        let report = compact(dir.path()).expect("compact");
        assert_eq!(report, Report { segments_removed: 0, oldest_index: None });
    }

    #[test]
    fn should_compact_respects_threshold() {
        assert!(!over_threshold(100, 256));
        assert!(!over_threshold(256, 256), "strictly greater than the threshold");
        assert!(over_threshold(257, 256));
    }

    #[test]
    fn total_bytes_sums_segments() {
        let dir = tempdir().expect("tempdir");
        {
            let mut writer = OplogWriter::open(dir.path()).expect("open");
            for byte in 0..3u8 {
                let _r = writer.append(msg("T1", byte)).expect("append");
            }
        }
        assert!(total_bytes(dir.path()).expect("total") > 0);
    }

    #[test]
    fn body_gc_eligible_honours_grace_window() {
        let grace = DEFAULT_GC_GRACE;
        // An in-flight spill: far younger than the barrier window — never GC'd.
        assert!(!body_gc_eligible(Duration::from_secs(1), grace));
        // Exactly at the grace boundary is still protected (strict greater-than).
        assert!(!body_gc_eligible(grace, grace));
        // A provable crash-orphan: older than any possible barrier window.
        assert!(body_gc_eligible(grace.saturating_add(Duration::from_secs(1)), grace));
    }
}
