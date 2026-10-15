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
use std::io::{self, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use cp_wire::framing;
use cp_wire::types::oplog::{OpEntry, OpEntryKind};
use cp_wire::types::snapshot::{Heads, SeenSet, Snapshot};

use crate::error::OplogResult;
use crate::replay::{self, Recovered, fold_entry};
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

    /// Set by the first failed write or sync. A failed `write_all` can leave a
    /// partial frame mid-segment, and a failed `fdatasync` can leave pages the
    /// kernel already dropped while a later sync reports success. Appending
    /// past either would acknowledge records that the next open truncates, so
    /// a poisoned writer refuses all further work until it is reopened (reopen
    /// truncates the torn tail and resumes from what is truly durable).
    poisoned: bool,

    /// Exclusive advisory lock on `dir/LOCK`, held for the writer's lifetime:
    /// two writers on one oplog would interleave frames.
    _lock: File,

    /// Test-only fault injection: the next frame write stops halfway and fails.
    #[cfg(test)]
    fail_next_write: bool,
}

/// Name of the lock file guarding single-writer access to an oplog directory.
const LOCK_FILE: &str = "LOCK";

/// Take the exclusive writer lock on `dir`, failing fast if another writer
/// (another process, or another handle in this one) already holds it.
fn lock_dir(dir: &Path) -> OplogResult<File> {
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join(LOCK_FILE))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(fs::TryLockError::WouldBlock) => Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            format!("oplog {} is already open by another writer", dir.display()),
        )
        .into()),
        Err(fs::TryLockError::Error(e)) => Err(e.into()),
    }
}

/// The error every call on a poisoned writer returns.
fn poisoned_error() -> io::Error {
    io::Error::other("oplog writer poisoned by an earlier I/O failure; reopen to recover")
}

impl OplogWriter {
    /// Open (creating if absent) the oplog in `dir` with the default segment
    /// size limit.
    ///
    /// # Errors
    ///
    /// Returns [`OplogError::Io`] if the directory or segment files cannot be
    /// created, read, scanned, or `fsync`'d.
    pub fn open<P>(dir: P) -> OplogResult<Self>
    where
        P: AsRef<Path>,
    {
        Self::open_with_segment_limit(dir, DEFAULT_SEGMENT_LIMIT)
    }

    /// Open the oplog with an explicit `segment_limit` (used by tests to force
    /// segment rolls without writing 64 MiB).
    ///
    /// # Errors
    ///
    /// Returns [`OplogError::Io`] for any filesystem failure during open,
    /// scan, torn-tail truncation, or initial-segment creation.
    pub fn open_with_segment_limit<P>(path: P, segment_limit: u64) -> OplogResult<Self>
    where
        P: AsRef<Path>,
    {
        let dir = path.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        let lock = lock_dir(&dir)?;

        let indices = segment::indices(&dir)?;
        // Running heads + the highest durable rev come from replaying the log.
        let recovered = replay::replay(&dir)?;
        let next_rev = recovered.rev_head.map_or(0, |rev| rev.wrapping_add(1));

        if let Some(index) = indices.last().copied() {
            let seg_path = segment::path(&dir, index);
            let scan = segment::read(&seg_path)?;
            let segment_has_record =
                scan.entries.iter().any(|entry| !matches!(entry.kind, OpEntryKind::Checkpoint { .. }));
            let mut file = OpenOptions::new().read(true).write(true).open(&seg_path)?;
            if scan.torn_tail {
                file.set_len(scan.valid_len)?;
                file.sync_data()?;
            }
            let _pos = file.seek(SeekFrom::Start(scan.valid_len))?;
            let mut writer = Self {
                dir,
                file,
                index,
                segment_bytes: scan.valid_len,
                segment_has_record,
                next_rev,
                segment_limit,
                state: recovered,
                poisoned: false,
                _lock: lock,
                #[cfg(test)]
                fail_next_write: false,
            };
            // A crash between creating a rolled segment and making its leading
            // checkpoint durable leaves it empty. Re-stamp it so the bounded-replay
            // invariant (every rolled segment opens with a checkpoint) holds.
            if index > 0 && scan.entries.is_empty() {
                let _cp_rev = writer.checkpoint()?;
            }
            Ok(writer)
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
                poisoned: false,
                _lock: lock,
                #[cfg(test)]
                fail_next_write: false,
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
        self.ensure_healthy()?;
        let entry = OpEntry::new(WRITER_SCHEMA_VERSION, self.next_rev, now_ms(), kind);
        let frame = framing::encode_entry(&entry)?;
        let frame_len = u64::try_from(frame.len()).unwrap_or(u64::MAX);

        if self.segment_has_record && self.segment_bytes.wrapping_add(frame_len) > self.segment_limit {
            // The roll's checkpoint takes this rev; re-frame under the next one.
            // Rolls happen once per segment, so the common path encodes once.
            self.roll()?;
            let reframed = OpEntry::new(WRITER_SCHEMA_VERSION, self.next_rev, entry.timestamp_ms, entry.kind);
            let reframed_bytes = framing::encode_entry(&reframed)?;
            return self.write_frame(&reframed, &reframed_bytes, true);
        }
        self.write_frame(&entry, &frame, true)
    }

    /// `fdatasync` the current segment, making every buffered append durable.
    ///
    /// This is the single point a group commit amortises: one `fdatasync`
    /// covers an entire batch of [`append_buffered`](Self::append_buffered)
    /// calls (design doc I2).
    ///
    /// # Errors
    ///
    /// Returns [`OplogError::Io`] if the `fdatasync` fails (which poisons the
    /// writer) or the writer is already poisoned.
    pub fn sync(&mut self) -> OplogResult<()> {
        self.ensure_healthy()?;
        if let Err(e) = self.file.sync_data() {
            self.poisoned = true;
            return Err(e.into());
        }
        Ok(())
    }

    /// Whether an earlier I/O failure has poisoned this writer.
    #[must_use]
    pub const fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    /// Refuse work once poisoned (see the `poisoned` field).
    fn ensure_healthy(&self) -> OplogResult<()> {
        if self.poisoned { Err(poisoned_error().into()) } else { Ok(()) }
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
    /// synced **before** returning: compaction treats a checkpoint-bearing
    /// segment as the replay base and deletes everything older, so that
    /// checkpoint must be durable before any compaction can observe it (one
    /// extra `fdatasync` per segment, i.e. per 64 MiB).
    fn roll(&mut self) -> OplogResult<()> {
        self.sync()?;
        let next_index = self.index.wrapping_add(1);
        let file = match Self::create_segment(&self.dir, next_index) {
            Ok(file) => file,
            Err(e) => {
                self.poisoned = true;
                return Err(e);
            }
        };
        self.file = file;
        self.index = next_index;
        self.segment_bytes = 0;
        self.segment_has_record = false;
        let snapshot = self.snapshot();
        let _cp_rev = self.write_record(OpEntryKind::Checkpoint { snapshot }, false)?;
        self.sync()
    }

    /// Low-level buffered append: frame, write, advance `rev`, fold state. Does
    /// **not** sync — durability is the caller's responsibility via
    /// [`sync`](Self::sync). `is_record` marks whether this is a real record (vs
    /// a checkpoint), which gates future rolls. This is the single place a `rev`
    /// is assigned.
    fn write_record(&mut self, kind: OpEntryKind, is_record: bool) -> OplogResult<u64> {
        self.ensure_healthy()?;
        let entry = OpEntry::new(WRITER_SCHEMA_VERSION, self.next_rev, now_ms(), kind);
        let frame = framing::encode_entry(&entry)?;
        self.write_frame(&entry, &frame, is_record)
    }

    /// Write an already-framed `entry` (whose `rev` must be `next_rev`) and
    /// advance the writer. A failed write poisons the writer: the segment may
    /// now hold a partial frame, and nothing may be appended after it.
    fn write_frame(&mut self, entry: &OpEntry, frame: &[u8], is_record: bool) -> OplogResult<u64> {
        let rev = entry.rev;
        if let Err(e) = self.write_bytes(frame) {
            self.poisoned = true;
            return Err(e.into());
        }

        self.segment_bytes = self.segment_bytes.wrapping_add(u64::try_from(frame.len()).unwrap_or(u64::MAX));
        self.next_rev = self.next_rev.wrapping_add(1);
        if is_record {
            self.segment_has_record = true;
        }
        fold_entry(&mut self.state, entry);
        Ok(rev)
    }

    /// The raw segment write (with test-only fault injection: write half the
    /// frame, then fail — a torn partial write).
    fn write_bytes(&mut self, frame: &[u8]) -> io::Result<()> {
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_write) {
            let half = frame.get(..frame.len().checked_shr(1).unwrap_or(0)).unwrap_or_default();
            self.file.write_all(half)?;
            return Err(io::Error::other("injected write failure"));
        }
        self.file.write_all(frame)
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

    /// A clone of the full recoverable snapshot (heads + seen-set + roster), as
    /// written into a checkpoint record.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        Snapshot::new(self.state.heads.clone(), self.state.seen.clone(), self.state.roster.clone())
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
mod tests;
