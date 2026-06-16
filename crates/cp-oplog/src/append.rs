//! [`OplogWriter`] — the single-writer, append-only, `fsync`'d log writer.
//!
//! The writer owns one open segment file, a monotonic `rev` counter, and a
//! running [`Heads`] snapshot it folds as it appends. Every
//! [`OplogWriter::append`] frames the entry (length prefix + CRC-32C), writes
//! it to the current segment, and `fdatasync`s **before** returning the
//! assigned `rev` — so a `rev` the caller has seen is always durable
//! (*announce-after-durable*, design doc K9). Segments roll when they would
//! exceed a size limit; the directory is `fsync`'d **only** when a new segment
//! file is created, never per append (design doc I2).
//!
//! # Leading checkpoints bound replay (design doc GAP 1 / I5)
//!
//! When the writer rolls to a new segment, it writes a **checkpoint** — a full
//! [`Heads`] snapshot — as that segment's **first record**. Recovery
//! ([`crate::replay`]) can then rebuild heads by reading only the newest
//! segment: its leading checkpoint is the base and the trailing records are the
//! only fold work. Replay cost is bounded by one segment, not the whole log.
//! The first segment (`seg-0`) has no leading checkpoint — there is no prior
//! state to snapshot — and replay handles that single-segment case with a
//! (still bounded) full fold.
//!
//! On [`OplogWriter::open`], a torn tail left by an interrupted write is
//! detected (CRC/length failure) and truncated away, so the log always resumes
//! at a clean record boundary (V1). The `rev` counter and running heads resume
//! from the durable state on disk, guaranteeing `rev`s are never reused and the
//! next checkpoint is accurate.
//!
//! # Synchronous vs buffered append (group commit)
//!
//! [`OplogWriter::append`] is *synchronous*: it writes and `fdatasync`s before
//! returning, so an announced `rev` is always durable. For amortised
//! throughput, the writer also exposes a **buffered** path:
//! [`OplogWriter::append_buffered`] writes the frame **without** syncing and
//! [`OplogWriter::sync`] flushes the current segment once. The off-loop
//! group-commit service ([`crate::service`]) drains a batch of records,
//! `append_buffered`s each, then calls `sync` **once** — one `fdatasync` per
//! group instead of per record. A buffered `rev` is *not* durable until the
//! next `sync`, so a caller must never announce it before then
//! (announce-after-durable, design doc K9). A segment roll flushes the old
//! segment durably before switching, so a batch that spans a roll still leaves
//! every record on a synced segment.

use std::fs::{self, File, OpenOptions};
use std::io::{Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use cp_wire::framing;
use cp_wire::types::oplog::{OpEntry, OpEntryKind};
use cp_wire::types::snapshot::{Heads, SeenSet, Snapshot};

use crate::error::OplogResult;
use crate::replay::{self, fold_entry, Recovered};
use crate::segment;

/// Default segment size limit: roll to a new segment once appending the next
/// record would push the current one past 64 MiB.
pub const DEFAULT_SEGMENT_LIMIT: u64 = 64 * 1024 * 1024;

/// Schema version stamped onto every [`OpEntry`] this writer emits.
const WRITER_SCHEMA_VERSION: u32 = 1;

/// Append-only writer for one oplog directory.
#[derive(Debug)]
pub struct OplogWriter {
    /// The oplog directory (holds the `seg-*.log` files).
    dir: PathBuf,

    /// The open handle to the current (newest) segment, positioned at its end.
    file: File,

    /// Index of the current segment.
    index: u64,

    /// Bytes written to the current segment so far.
    segment_bytes: u64,

    /// Whether the current segment holds at least one non-checkpoint record.
    /// A segment is never rolled before it carries a real record, so a single
    /// oversized record cannot thrash empty checkpoint-only segments.
    segment_has_record: bool,

    /// The `rev` that the next append will assign.
    next_rev: u64,

    /// Roll to a new segment once a write would push `segment_bytes` past this.
    segment_limit: u64,

    /// Running recoverable state (heads + seen-set), folded as records are
    /// appended. Seeded on open by replaying the durable log; its heads and
    /// seen-set are written verbatim into each segment's leading checkpoint.
    state: Recovered,
}

impl OplogWriter {
    /// Open (creating if absent) the oplog in `dir` with the default segment
    /// size limit.
    ///
    /// # Errors
    ///
    /// Returns [`OplogError::Io`] if the directory or segment files cannot be
    /// created, read, scanned, or `fsync`'d.
    pub fn open<P: AsRef<Path>>(dir: P) -> OplogResult<Self> {
        Self::open_with_segment_limit(dir, DEFAULT_SEGMENT_LIMIT)
    }

    /// Open the oplog with an explicit `segment_limit` (used by tests to force
    /// segment rolls without writing 64 MiB).
    ///
    /// # Errors
    ///
    /// Returns [`OplogError::Io`] for any filesystem failure during open,
    /// scan, torn-tail truncation, or initial-segment creation.
    pub fn open_with_segment_limit<P: AsRef<Path>>(dir: P, segment_limit: u64) -> OplogResult<Self> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;

        let indices = segment::indices(&dir)?;
        // Running heads + the highest durable rev come from replaying the log.
        let recovered = replay::replay(&dir)?;
        let next_rev = recovered.rev_head.map_or(0, |rev| rev.wrapping_add(1));

        if let Some(index) = indices.last().copied() {
            let path = segment::path(&dir, index);
            let scan = segment::read(&path)?;
            let segment_has_record = scan
                .entries
                .iter()
                .any(|entry| !matches!(entry.kind, OpEntryKind::Checkpoint { .. }));
            let mut file = OpenOptions::new().read(true).write(true).open(&path)?;
            if scan.torn_tail {
                file.set_len(scan.valid_len)?;
                file.sync_data()?;
            }
            let _pos = file.seek(SeekFrom::Start(scan.valid_len))?;
            Ok(Self {
                dir,
                file,
                index,
                segment_bytes: scan.valid_len,
                segment_has_record,
                next_rev,
                segment_limit,
                state: recovered,
            })
        } else {
            // Fresh oplog: create segment 0 (no leading checkpoint — there is
            // no prior state to snapshot) and durably link it into the dir.
            let file = Self::create_segment(&dir, 0)?;
            Ok(Self {
                dir,
                file,
                index: 0,
                segment_bytes: 0,
                segment_has_record: false,
                next_rev,
                segment_limit,
                state: recovered,
            })
        }
    }

    /// Create segment `index`, `fsync`-ing the directory so the new file's
    /// directory entry is itself durable (design doc I2: dir fsync on segment
    /// creation only).
    fn create_segment(dir: &Path, index: u64) -> OplogResult<File> {
        let path = segment::path(dir, index);
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(true).open(path)?;
        Self::sync_dir(dir)?;
        Ok(file)
    }

    /// `fsync` a directory handle so a freshly-created child file is durable.
    fn sync_dir(dir: &Path) -> OplogResult<()> {
        let handle = File::open(dir)?;
        handle.sync_all()?;
        Ok(())
    }

    /// Append one record synchronously, returning its durably-assigned `rev`.
    ///
    /// Equivalent to [`append_buffered`](Self::append_buffered) followed by
    /// [`sync`](Self::sync): the bytes are framed, written, and `fdatasync`'d
    /// before the `rev` is returned, so the caller never observes a `rev` that a
    /// crash could undo. If the current segment is full, the writer first rolls
    /// to a new segment and stamps its leading checkpoint (GAP 1).
    ///
    /// # Errors
    ///
    /// Returns [`OplogError::Frame`] if the entry cannot be framed, or
    /// [`OplogError::Io`] if the write, sync, or a segment roll fails.
    pub fn append(&mut self, kind: OpEntryKind) -> OplogResult<u64> {
        let rev = self.append_buffered(kind)?;
        self.sync()?;
        Ok(rev)
    }

    /// Append one record **without** syncing, returning its assigned `rev`.
    ///
    /// The frame is written and the `rev` assigned, but the bytes are not yet
    /// durable — the caller **must** call [`sync`](Self::sync) before announcing
    /// the `rev` (announce-after-durable, design doc K9). This is the primitive
    /// the group-commit service ([`crate::service`]) batches: many buffered
    /// appends share a single `fdatasync`. A segment roll triggered here flushes
    /// the old segment durably first (see [`roll`](Self::roll)).
    ///
    /// # Errors
    ///
    /// Returns [`OplogError::Frame`] if the entry cannot be framed, or
    /// [`OplogError::Io`] if the write or a segment roll fails.
    pub fn append_buffered(&mut self, kind: OpEntryKind) -> OplogResult<u64> {
        // Probe the framed size to decide whether this record would overflow
        // the segment. (The probe re-encodes; `rev`/timestamp do not affect the
        // decision, so a zeroed placeholder is fine.)
        let probe = OpEntry {
            schema_version: WRITER_SCHEMA_VERSION,
            rev: self.next_rev,
            timestamp_ms: 0,
            kind: kind.clone(),
        };
        let frame_len = framing::encode_entry(&probe)?.len() as u64;

        if self.segment_has_record && self.segment_bytes.wrapping_add(frame_len) > self.segment_limit
        {
            self.roll()?;
        }
        self.write_record(kind, true)
    }

    /// `fdatasync` the current segment, making every buffered append durable.
    ///
    /// This is the single point a group commit amortises: one `fdatasync`
    /// covers an entire batch of [`append_buffered`](Self::append_buffered)
    /// calls (design doc I2).
    ///
    /// # Errors
    ///
    /// Returns [`OplogError::Io`] if the `fdatasync` fails.
    pub fn sync(&self) -> OplogResult<()> {
        self.file.sync_data()?;
        Ok(())
    }

    /// Force a checkpoint (a full [`Heads`] snapshot) into the current segment.
    ///
    /// Mostly emitted automatically on segment roll; exposed so a future
    /// cadence-driven checkpointer (or a test) can bound replay length within a
    /// long-lived segment too.
    ///
    /// # Errors
    ///
    /// Returns [`OplogError::Frame`] or [`OplogError::Io`] on a framing or I/O
    /// failure.
    pub fn checkpoint(&mut self) -> OplogResult<u64> {
        let snapshot = self.snapshot();
        let rev = self.write_record(OpEntryKind::Checkpoint { snapshot }, false)?;
        self.sync()?;
        Ok(rev)
    }

    /// Roll to a fresh segment and stamp its leading checkpoint.
    ///
    /// The current (old) segment is `fdatasync`'d **before** the switch, so any
    /// buffered-but-unsynced records on it become durable at the roll boundary
    /// — a group-commit batch that spans a roll never leaves records on an
    /// unsynced, abandoned segment. The new segment's leading checkpoint is
    /// written buffered; the caller's trailing `sync` (or the next group commit)
    /// makes it durable.
    fn roll(&mut self) -> OplogResult<()> {
        self.sync()?;
        let next_index = self.index.wrapping_add(1);
        let file = Self::create_segment(&self.dir, next_index)?;
        self.file = file;
        self.index = next_index;
        self.segment_bytes = 0;
        self.segment_has_record = false;
        let snapshot = self.snapshot();
        let _cp_rev = self.write_record(OpEntryKind::Checkpoint { snapshot }, false)?;
        Ok(())
    }

    /// Low-level buffered append: frame, write, advance `rev`, fold state. Does
    /// **not** sync — durability is the caller's responsibility via
    /// [`sync`](Self::sync). `is_record` marks whether this is a real record (vs
    /// a checkpoint), which gates future rolls. This is the single place a `rev`
    /// is assigned.
    fn write_record(&mut self, kind: OpEntryKind, is_record: bool) -> OplogResult<u64> {
        let rev = self.next_rev;
        let entry =
            OpEntry { schema_version: WRITER_SCHEMA_VERSION, rev, timestamp_ms: now_ms(), kind };
        let frame = framing::encode_entry(&entry)?;

        self.file.write_all(&frame)?;

        self.segment_bytes = self.segment_bytes.wrapping_add(frame.len() as u64);
        self.next_rev = self.next_rev.wrapping_add(1);
        if is_record {
            self.segment_has_record = true;
        }
        fold_entry(&mut self.state, &entry);
        Ok(rev)
    }

    /// The `rev` the next append will assign.
    #[must_use]
    pub const fn next_rev(&self) -> u64 {
        self.next_rev
    }

    /// The index of the current (newest) segment.
    #[must_use]
    pub const fn current_segment(&self) -> u64 {
        self.index
    }

    /// The writer's running head snapshot.
    #[must_use]
    pub const fn heads(&self) -> &Heads {
        &self.state.heads
    }

    /// The writer's running dedup seen-set.
    #[must_use]
    pub const fn seen(&self) -> &SeenSet {
        &self.state.seen
    }

    /// A clone of the full recoverable snapshot (heads + seen-set), as written
    /// into a checkpoint record.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        Snapshot { heads: self.state.heads.clone(), seen: self.state.seen.clone() }
    }
}

/// Wall-clock milliseconds since the Unix epoch, or `0` if the clock is set
/// before the epoch (the value is informational only — `rev` is the authority).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_wire::types::Phase;
    use tempfile::tempdir;

    fn phase_kind() -> OpEntryKind {
        OpEntryKind::PhaseTransition { phase: Phase::Streaming }
    }

    #[test]
    fn append_assigns_monotonic_revs() {
        let dir = tempdir().expect("tempdir");
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        let revs: Vec<u64> = (0..10).map(|_unused| writer.append(phase_kind()).expect("append")).collect();
        assert_eq!(revs, (0..10).collect::<Vec<_>>(), "revs strictly increase from 0");
    }

    #[test]
    fn revs_resume_across_reopen_without_reuse() {
        let dir = tempdir().expect("tempdir");
        {
            let mut writer = OplogWriter::open(dir.path()).expect("open");
            for _unused in 0..5 {
                let _rev = writer.append(phase_kind()).expect("append");
            }
            assert_eq!(writer.next_rev(), 5);
        }
        // Reopen: next rev must continue at 5, never reusing 0..4.
        let mut writer = OplogWriter::open(dir.path()).expect("reopen");
        assert_eq!(writer.next_rev(), 5, "rev resumes past the highest durable rev");
        let rev = writer.append(phase_kind()).expect("append");
        assert_eq!(rev, 5);
    }

    #[test]
    fn appended_records_read_back_in_order() {
        let dir = tempdir().expect("tempdir");
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        for _unused in 0..4 {
            let _rev = writer.append(phase_kind()).expect("append");
        }
        let path = segment::path(dir.path(), 0);
        let scan = segment::read(&path).expect("read");
        assert_eq!(scan.entries.len(), 4);
        assert!(!scan.torn_tail);
        let revs: Vec<u64> = scan.entries.iter().map(|e| e.rev).collect();
        assert_eq!(revs, vec![0, 1, 2, 3]);
    }

    #[test]
    fn small_segment_limit_forces_roll_and_continues_revs() {
        let dir = tempdir().expect("tempdir");
        // A tiny limit forces a new segment after roughly each record. Each
        // roll injects a leading checkpoint, so revs advance by more than the
        // user-record count — assert monotonicity + a roll, not an exact total.
        let mut writer = OplogWriter::open_with_segment_limit(dir.path(), 16).expect("open");
        let mut last = 0;
        for _unused in 0..6 {
            last = writer.append(phase_kind()).expect("append");
        }
        assert!(writer.current_segment() > 0, "tiny limit must have rolled segments");
        let after_first_session = writer.next_rev();
        assert!(after_first_session > last, "next_rev is past the last appended rev");

        // Reopen across multiple segments: rev resumes exactly where it stopped.
        let writer = OplogWriter::open_with_segment_limit(dir.path(), 16).expect("reopen");
        assert_eq!(
            writer.next_rev(),
            after_first_session,
            "rev resumes across multi-segment scan, checkpoints included",
        );
    }

    #[test]
    fn torn_tail_is_truncated_on_open() {
        let dir = tempdir().expect("tempdir");
        {
            let mut writer = OplogWriter::open(dir.path()).expect("open");
            let _rev = writer.append(phase_kind()).expect("append");
            let _rev = writer.append(phase_kind()).expect("append");
        }
        // Corrupt the segment by appending garbage bytes (a torn write).
        let path = segment::path(dir.path(), 0);
        {
            let mut f = OpenOptions::new().append(true).open(&path).expect("reopen for corrupt");
            f.write_all(&[0xFF, 0xFF, 0xFF, 0xFF, 0x01]).expect("write garbage");
            f.sync_data().expect("sync");
        }
        // Reopen: the garbage tail is dropped, rev resumes at 2.
        let mut writer = OplogWriter::open(dir.path()).expect("reopen");
        assert_eq!(writer.next_rev(), 2, "garbage tail must not advance rev");
        let rev = writer.append(phase_kind()).expect("append after recovery");
        assert_eq!(rev, 2);

        let scan = segment::read(&path).expect("read");
        assert_eq!(scan.entries.len(), 3, "two survivors + one fresh append");
        assert!(!scan.torn_tail, "tail is clean after recovery");
    }

    #[test]
    fn explicit_checkpoint_carries_running_heads() {
        let dir = tempdir().expect("tempdir");
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        let _r = writer
            .append(OpEntryKind::MessageCreated {
                thread_id: "T1".to_owned(),
                message_id: "m1".to_owned(),
                head: cp_wire::types::ContentHash::new([0x77; 32]),
            })
            .expect("append");
        let _cp = writer.checkpoint().expect("checkpoint");

        // The newest segment now ends with a checkpoint whose heads match the
        // running heads.
        let scan = segment::read(&segment::path(dir.path(), 0)).expect("read");
        let last = scan.entries.last().expect("entries");
        match &last.kind {
            OpEntryKind::Checkpoint { snapshot } => assert_eq!(&snapshot.heads, writer.heads()),
            other => panic!("expected trailing checkpoint, got {other:?}"),
        }
    }
}
