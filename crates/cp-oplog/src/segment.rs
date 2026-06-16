//! Segment files — discovery, naming, and torn-tail-aware frame reading.
//!
//! The oplog is stored as a sequence of **segment** files in one directory,
//! named `seg-{n}.log` with a zero-padded, lexicographically-sortable index.
//! Splitting into segments bounds the cost of compaction (an old segment whose
//! every record is past the acknowledged-`rev` barrier can be dropped whole)
//! and the `fsync(dir)` cost (paid only when a *new* segment file is created,
//! not per append).
//!
//! Reading is **torn-tail aware**: [`read`] decodes records until it hits the
//! end of valid data — either clean EOF, or a partial/corrupt frame left by an
//! interrupted write. It reports the byte offset of the last good record
//! boundary so the writer can truncate the garbage tail away (design doc V1).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cp_wire::framing::{self, FrameError};
use cp_wire::types::oplog::OpEntry;

/// Zero-padding width for the segment index in `seg-{n}.log`.
///
/// 20 digits holds the full `u64` range while keeping filenames sortable as
/// plain strings (the directory listing order then matches numeric order).
const INDEX_WIDTH: usize = 20;

/// Filename prefix for segment files.
const PREFIX: &str = "seg-";

/// Filename suffix for segment files.
const SUFFIX: &str = ".log";

/// The decoded contents of a segment up to its last valid record boundary.
#[derive(Debug, Default)]
pub struct Scan {
    /// Every entry that decoded cleanly, in file order.
    pub entries: Vec<OpEntry>,

    /// Byte offset of the first byte *after* the last valid record. Equal to
    /// the file length for an intact segment; smaller when a torn/corrupt tail
    /// was found and should be truncated away.
    pub valid_len: u64,

    /// `true` if a partial or corrupt trailing frame was detected (the bytes
    /// in `[valid_len, file_len)` are garbage from an interrupted write).
    pub torn_tail: bool,
}

/// Build the on-disk path of segment number `index` within `dir`.
#[must_use]
pub fn path(dir: &Path, index: u64) -> PathBuf {
    dir.join(format!("{PREFIX}{index:0INDEX_WIDTH$}{SUFFIX}"))
}

/// Parse a segment index out of a file name, or `None` if it is not a segment.
fn parse_index(name: &str) -> Option<u64> {
    let middle = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
    middle.parse::<u64>().ok()
}

/// List the existing segment indices in `dir`, ascending.
///
/// Returns an empty vector if the directory does not yet exist.
///
/// # Errors
///
/// Propagates I/O errors from reading the directory (other than
/// not-found, which yields an empty list).
pub fn indices(dir: &Path) -> io::Result<Vec<u64>> {
    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut found = Vec::new();
    for entry in read_dir {
        let entry = entry?;
        if let Some(name) = entry.file_name().to_str()
            && let Some(index) = parse_index(name)
        {
            found.push(index);
        }
    }
    found.sort_unstable();
    Ok(found)
}

/// Read a segment file, decoding records until clean EOF or a torn tail.
///
/// The whole file is read into memory (segments are size-bounded), then walked
/// frame by frame. Decoding stops at the first frame that cannot be fully and
/// validly decoded; everything before it is returned, along with the offset of
/// that clean boundary (see [`Scan`]).
///
/// # Errors
///
/// Propagates I/O errors from reading the file. Decode failures are *not*
/// errors — they mark the torn tail and end the scan.
pub fn read(path: &Path) -> io::Result<Scan> {
    let data = fs::read(path)?;
    Ok(scan_bytes(&data))
}

/// Walk a framed byte buffer, collecting entries up to the first decode
/// failure. Factored out of [`read`] so it can be unit-tested without touching
/// the filesystem.
fn scan_bytes(data: &[u8]) -> Scan {
    let mut scan = Scan::default();
    let mut offset: usize = 0;

    while let Some(remaining) = data.get(offset..) {
        if remaining.is_empty() {
            break;
        }
        match framing::decode_entry(remaining) {
            Ok((entry, consumed)) => {
                scan.entries.push(entry);
                offset = offset.wrapping_add(consumed);
            }
            Err(FrameError::Incomplete | FrameError::CrcMismatch { .. } | FrameError::DeserializeError(_) | FrameError::PayloadTooLarge(_) | FrameError::SerializeError(_)) => {
                scan.torn_tail = true;
                break;
            }
        }
    }

    scan.valid_len = offset as u64;
    scan
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_wire::framing::encode_entry;
    use cp_wire::types::oplog::OpEntryKind;
    use cp_wire::types::Phase;

    fn entry(rev: u64) -> OpEntry {
        OpEntry {
            schema_version: 1,
            rev,
            timestamp_ms: 0,
            kind: OpEntryKind::PhaseTransition { phase: Phase::Idle },
        }
    }

    #[test]
    fn path_is_zero_padded_and_sortable() {
        let dir = Path::new("/tmp/oplog");
        let p9 = path(dir, 9);
        let p10 = path(dir, 10);
        assert!(p9 < p10, "lexicographic order must match numeric order");
        assert!(p9.to_string_lossy().contains("seg-00000000000000000009.log"));
    }

    #[test]
    fn parse_round_trips_index() {
        assert_eq!(parse_index("seg-00000000000000000042.log"), Some(42));
        assert_eq!(parse_index("not-a-segment.txt"), None);
        assert_eq!(parse_index("seg-xx.log"), None);
    }

    #[test]
    fn scan_reads_all_clean_frames() {
        let mut buf = Vec::new();
        for rev in 0..5 {
            buf.extend(encode_entry(&entry(rev)).expect("encode"));
        }
        let scan = scan_bytes(&buf);
        assert_eq!(scan.entries.len(), 5);
        assert_eq!(scan.valid_len, buf.len() as u64);
        assert!(!scan.torn_tail);
    }

    #[test]
    fn scan_stops_at_torn_tail() {
        let mut buf = encode_entry(&entry(0)).expect("encode");
        let clean_len = buf.len();
        // Append a second frame, then truncate it mid-payload.
        buf.extend(encode_entry(&entry(1)).expect("encode"));
        buf.truncate(buf.len().wrapping_sub(3));

        let scan = scan_bytes(&buf);
        assert_eq!(scan.entries.len(), 1);
        assert_eq!(scan.valid_len, clean_len as u64);
        assert!(scan.torn_tail);
    }

    #[test]
    fn scan_flags_corrupt_tail() {
        let mut buf = encode_entry(&entry(0)).expect("encode");
        let clean_len = buf.len();
        buf.extend(encode_entry(&entry(1)).expect("encode"));
        // Flip a payload byte in the second frame to break its CRC.
        let last = buf.len().wrapping_sub(1);
        if let Some(byte) = buf.get_mut(last) {
            *byte ^= 0xFF;
        }
        let scan = scan_bytes(&buf);
        assert_eq!(scan.entries.len(), 1);
        assert_eq!(scan.valid_len, clean_len as u64);
        assert!(scan.torn_tail);
    }

    #[test]
    fn scan_empty_is_clean() {
        let scan = scan_bytes(&[]);
        assert!(scan.entries.is_empty());
        assert_eq!(scan.valid_len, 0);
        assert!(!scan.torn_tail);
    }
}
