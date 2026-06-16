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
//! This is a *synchronous* writer: `append` performs the `fdatasync` inline.
//! The off-loop group-commit thread that amortizes syncs across many records
//! arrives in a later phase; the durability contract it must preserve is
//! defined and tested here first.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use cp_wire::framing::{self, FrameError};
use cp_wire::types::heads::Heads;
use cp_wire::types::oplog::{OpEntry, OpEntryKind};

use crate::replay::{self, fold_entry};
use crate::segment;

/// Default segment size limit: roll to a new segment once appending the next
/// record would push the current one past 64 MiB.
pub const DEFAULT_SEGMENT_LIMIT: u64 = 64 * 1024 * 1024;

/// Schema version stamped onto every [`OpEntry`] this writer emits.
const WRITER_SCHEMA_VERSION: u32 = 1;

/// An error from oplog write operations.
#[derive(Debug)]
pub enum OplogError {
    /// An underlying filesystem operation failed (open, write, sync, …).
    Io(io::Error),

    /// The entry could not be framed (serialisation failed or it was too
    /// large to carry a 32-bit length prefix).
    Frame(FrameError),
}

impl fmt::Display for OplogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "oplog I/O error: {e}"),
            Self::Frame(e) => write!(f, "oplog framing error: {e}"),
        }
    }
}

impl From<io::Error> for OplogError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<FrameError> for OplogError {
    fn from(e: FrameError) -> Self {
        Self::Frame(e)
    }
}

/// A `Result` whose error is an [`OplogError`].
pub type OplogResult<T> = Result<T, OplogError>;

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

    /// Running head snapshot, folded as records are appended. Seeded on open by
    /// replaying the durable log; written verbatim into each segment's leading
    /// checkpoint.
    heads: Heads,
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
                heads: recovered.heads,
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
                heads: recovered.heads,
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

    /// Append one record, returning its durably-assigned `rev`.
    ///
    /// The bytes are framed, written, and `fdatasync`'d before the `rev` is
    /// returned, so the caller never observes a `rev` that a crash could undo.
    /// If the current segment is full, the writer first rolls to a new segment
    /// and stamps its leading checkpoint (GAP 1) — the checkpoint and this
    /// record both land durably.
    ///
    /// # Errors
    ///
    /// Returns [`OplogError::Frame`] if the entry cannot be framed, or
    /// [`OplogError::Io`] if the write, sync, or a segment roll fails.
    pub fn append(&mut self, kind: OpEntryKind) -> OplogResult<u64> {
        // Probe the framed size to decide whether this record would overflow
        // the segment. (The probe re-encodes; a later phase amortises this with
        // the group-commit path. `rev`/timestamp do not affect the decision.)
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
        let snapshot = self.heads.clone();
        self.write_record(OpEntryKind::Checkpoint { heads: snapshot }, false)
    }

    /// Roll to a fresh segment and stamp its leading checkpoint.
    fn roll(&mut self) -> OplogResult<()> {
        let next_index = self.index.wrapping_add(1);
        let file = Self::create_segment(&self.dir, next_index)?;
        self.file = file;
        self.index = next_index;
        self.segment_bytes = 0;
        self.segment_has_record = false;
        let snapshot = self.heads.clone();
        let _cp_rev = self.write_record(OpEntryKind::Checkpoint { heads: snapshot }, false)?;
        Ok(())
    }

    /// Low-level append: frame, write, `fdatasync`, advance `rev`, fold heads.
    /// `is_record` marks whether this is a real record (vs a checkpoint), which
    /// gates future rolls. This is the single place a `rev` is assigned.
    fn write_record(&mut self, kind: OpEntryKind, is_record: bool) -> OplogResult<u64> {
        let rev = self.next_rev;
        let entry =
            OpEntry { schema_version: WRITER_SCHEMA_VERSION, rev, timestamp_ms: now_ms(), kind };
        let frame = framing::encode_entry(&entry)?;

        self.file.write_all(&frame)?;
        self.file.sync_data()?;

        self.segment_bytes = self.segment_bytes.wrapping_add(frame.len() as u64);
        self.next_rev = self.next_rev.wrapping_add(1);
        if is_record {
            self.segment_has_record = true;
        }
        fold_entry(&mut self.heads, &entry);
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
        &self.heads
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
            OpEntryKind::Checkpoint { heads } => assert_eq!(heads, writer.heads()),
            other => panic!("expected trailing checkpoint, got {other:?}"),
        }
    }
}
