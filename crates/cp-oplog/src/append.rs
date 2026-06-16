//! [`OplogWriter`] — the single-writer, append-only, `fsync`'d log writer.
//!
//! The writer owns one open segment file and a monotonic `rev` counter. Every
//! [`OplogWriter::append`] frames the entry (length prefix + CRC-32C), writes
//! it to the current segment, and `fdatasync`s **before** returning the
//! assigned `rev` — so a `rev` the caller has seen is always durable
//! (*announce-after-durable*, design doc K9). Segments roll when they would
//! exceed a size limit; the directory is `fsync`'d **only** when a new segment
//! file is created, never per append (design doc I2).
//!
//! On [`OplogWriter::open`], a torn tail left by an interrupted write is
//! detected (CRC/length failure) and truncated away, so the log always resumes
//! at a clean record boundary (V1). The `rev` counter resumes from one past the
//! highest durable `rev` found on disk, guaranteeing `rev`s are never reused.
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
use cp_wire::types::oplog::{OpEntry, OpEntryKind};

use crate::segment;

/// Default segment size limit: roll to a new segment once appending the next
/// record would push the current one past 64 MiB.
pub const DEFAULT_SEGMENT_LIMIT: u64 = 64 * 1024 * 1024;

/// Schema version stamped onto every [`OpEntry`] this writer emits.
const WRITER_SCHEMA_VERSION: u32 = 1;

/// Where a reopened oplog resumes from: the highest durable `rev` found (if
/// any) and the newest segment index (if any segments exist).
type Resume = (Option<u64>, Option<u64>);

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

    /// The `rev` that the next [`append`](OplogWriter::append) will assign.
    next_rev: u64,

    /// Roll to a new segment once a write would push `segment_bytes` past this.
    segment_limit: u64,
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

        let segment_indices = segment::indices(&dir)?;
        let (max_rev, last_index) = Self::scan_for_resume(&dir, &segment_indices)?;
        let next_rev = max_rev.map_or(0, |rev| rev.wrapping_add(1));

        if let Some(index) = last_index {
            let path = segment::path(&dir, index);
            let scan = segment::read(&path)?;
            let mut file = OpenOptions::new().read(true).write(true).open(&path)?;
            if scan.torn_tail {
                file.set_len(scan.valid_len)?;
                file.sync_data()?;
            }
            let _pos = file.seek(SeekFrom::Start(scan.valid_len))?;
            Ok(Self { dir, file, index, segment_bytes: scan.valid_len, next_rev, segment_limit })
        } else {
            // Fresh oplog: create segment 0 and durably link it into dir.
            let file = Self::create_segment(&dir, 0)?;
            Ok(Self { dir, file, index: 0, segment_bytes: 0, next_rev, segment_limit })
        }
    }

    /// Scan every segment to find the highest durable `rev` and the newest
    /// segment index (see [`Resume`]). Each component is `None` when the oplog
    /// has no segments / no entries yet.
    fn scan_for_resume(dir: &Path, segment_indices: &[u64]) -> OplogResult<Resume> {
        let last_index = segment_indices.last().copied();
        let mut max_rev: Option<u64> = None;
        for &index in segment_indices {
            let scan = segment::read(&segment::path(dir, index))?;
            for entry in &scan.entries {
                max_rev = Some(max_rev.map_or(entry.rev, |current| current.max(entry.rev)));
            }
        }
        Ok((max_rev, last_index))
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
    ///
    /// # Errors
    ///
    /// Returns [`OplogError::Frame`] if the entry cannot be framed, or
    /// [`OplogError::Io`] if the write, sync, or a segment roll fails.
    pub fn append(&mut self, kind: OpEntryKind) -> OplogResult<u64> {
        let rev = self.next_rev;
        let entry = OpEntry { schema_version: WRITER_SCHEMA_VERSION, rev, timestamp_ms: now_ms(), kind };
        let frame = framing::encode_entry(&entry)?;
        let frame_len = frame.len() as u64;

        self.maybe_roll(frame_len)?;

        self.file.write_all(&frame)?;
        self.file.sync_data()?;

        self.segment_bytes = self.segment_bytes.wrapping_add(frame_len);
        self.next_rev = self.next_rev.wrapping_add(1);
        Ok(rev)
    }

    /// Roll to a fresh segment if writing `incoming` more bytes would push the
    /// current segment past its limit. A non-empty segment is required before
    /// rolling, so a single oversized record never spins on empty segments.
    fn maybe_roll(&mut self, incoming: u64) -> OplogResult<()> {
        let would_be = self.segment_bytes.wrapping_add(incoming);
        if self.segment_bytes > 0 && would_be > self.segment_limit {
            let next_index = self.index.wrapping_add(1);
            let file = Self::create_segment(&self.dir, next_index)?;
            self.file = file;
            self.index = next_index;
            self.segment_bytes = 0;
        }
        Ok(())
    }

    /// The `rev` the next [`append`](OplogWriter::append) will assign.
    #[must_use]
    pub const fn next_rev(&self) -> u64 {
        self.next_rev
    }

    /// The index of the current (newest) segment.
    #[must_use]
    pub const fn current_segment(&self) -> u64 {
        self.index
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
        // A tiny limit forces a new segment after roughly each record.
        let mut writer = OplogWriter::open_with_segment_limit(dir.path(), 16).expect("open");
        for _unused in 0..6 {
            let _rev = writer.append(phase_kind()).expect("append");
        }
        assert!(writer.current_segment() > 0, "tiny limit must have rolled segments");
        assert_eq!(writer.next_rev(), 6);

        // Reopen across multiple segments: rev still resumes correctly.
        let writer = OplogWriter::open_with_segment_limit(dir.path(), 16).expect("reopen");
        assert_eq!(writer.next_rev(), 6, "rev resumes across multi-segment scan");
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
}
