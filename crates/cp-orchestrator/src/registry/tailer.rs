//! [`Tailer`] — the backend's incremental, gap-free consumer of one agent's
//! oplog directory.
//!
//! Each [`poll`](Tailer::poll) returns only the entries appended since the
//! previous call, in `rev` order. The `rev` is monotonic and no entry is
//! skipped, so the consumer sees a complete, ordered event stream. The tailer
//! is a **pure poll primitive** — no kernel watch, no thread. The live driver
//! (inotify + backstop timer) belongs to the runtime loop, exactly as
//! [`AgentRegistry`](crate::registry::AgentRegistry) is a pure scan+diff driven
//! by an external cadence.
//!
//! Split out of `channel.rs` (which retains the read/write
//! [`AgentChannel`](crate::registry::channel::AgentChannel)) to keep each file
//! within the workspace's per-file line budget; the `Tailer` remains reachable
//! at the stable `channel::Tailer` path via a re-export there.

use std::fs;
use std::io;
use std::path::PathBuf;

use cp_oplog::segment;
use cp_wire::types::oplog::OpEntry;

/// Incremental, gap-free consumer of an agent's oplog directory.
///
/// Remembers the highest delivered `rev` and the segment index it was in, so
/// each [`poll`](Tailer::poll) reads only the newest segment(s) and returns
/// only entries the consumer has not yet seen. Correct across segment rolls,
/// compaction (which deletes only segments the tailer has already passed), and
/// a missing directory (yields an empty poll, not an error).
#[derive(Debug)]
pub struct Tailer {
    /// The agent's oplog directory (`<folder>/oplog`).
    dir: PathBuf,

    /// The segment index we last read entries from. On the next poll we start
    /// scanning from this index (skipping older segments entirely).
    last_index: Option<u64>,

    /// Byte offset already consumed within [`last_index`](Self::last_index)'s
    /// segment file — the running cursor that makes [`poll`](Self::poll)
    /// **incremental**. Each poll reads only the bytes appended past this
    /// offset (and skips the read entirely when the file length is unchanged),
    /// instead of re-reading and re-deserializing the entire current segment
    /// every tick. Reset to `0` whenever the cursor advances to a newer segment
    /// (a roll). `0` until the first segment is read.
    last_offset: u64,

    /// The highest `rev` delivered to the consumer. Entries at or below this
    /// rev are filtered out, ensuring gap-free, exactly-once delivery.
    last_rev: Option<u64>,
}

impl Tailer {
    /// Create a tailer over `oplog_dir`. The first [`poll`](Tailer::poll)
    /// returns every entry in the log (a full catch-up); use
    /// [`seed`](Tailer::seed) first to skip already-processed history.
    #[must_use]
    pub fn new(oplog_dir: PathBuf) -> Self {
        Self { dir: oplog_dir, last_index: None, last_offset: 0, last_rev: None }
    }

    /// Advance the cursor to `rev` so the next poll skips everything at or
    /// below it. Call after replaying the log to a known point.
    pub fn seed(&mut self, rev: u64) {
        self.last_rev = Some(rev);
    }

    /// Read new entries since the last poll, advancing the cursor.
    ///
    /// Returns entries in ascending `rev` order. An empty `Vec` means no new
    /// entries were appended since the last call. The method is idempotent: two
    /// consecutive calls with no intervening agent writes yield `[]` then `[]`.
    ///
    /// # Incremental — O(new bytes), not O(segment size)
    ///
    /// The cursor tracks both the newest segment index **and the byte offset
    /// already consumed within it** ([`last_offset`](Self::last_offset)). Each
    /// poll:
    ///
    /// 1. Skips segments older than the cursor entirely.
    /// 2. For the segment the cursor sits in, first compares the file length to
    ///    the consumed offset via a single `stat` — if unchanged, **nothing was
    ///    appended and the file is never read or parsed** (the overwhelmingly
    ///    common idle case). This is what stops the backend burning a core
    ///    re-deserializing an unchanged segment every tick.
    /// 3. Otherwise reads the file and frame-decodes **only the appended tail**
    ///    (`bytes[offset..]`) — frames are self-delimiting, so scanning from the
    ///    previous clean record boundary is correct — then advances the offset
    ///    by the clean length of that tail (a torn final frame leaves the offset
    ///    at its boundary, so the now-complete frame is picked up next poll).
    /// 4. A newer segment (a roll) resets the in-segment offset to `0`.
    ///
    /// The `last_rev` high-water filter is retained as a belt-and-braces
    /// exactly-once guard (a re-read after a torn tail can never re-deliver).
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if a segment file cannot be listed or read.
    pub fn poll(&mut self) -> io::Result<Vec<OpEntry>> {
        let indices = segment::indices(&self.dir)?;
        let cursor_index = self.last_index.unwrap_or(0);
        let mut new_entries: Vec<OpEntry> = Vec::new();

        for &index in &indices {
            if index < cursor_index {
                continue; // fully-consumed historical segment.
            }

            // Byte offset already consumed in THIS segment: the running cursor
            // for the segment we're mid-way through, else 0 for a fresh (rolled)
            // segment we haven't touched.
            let base_offset = if Some(index) == self.last_index { self.last_offset } else { 0 };

            let path = segment::path(&self.dir, index);

            // Cheap idle skip: a single `stat`. If no bytes were appended past
            // what we've consumed, do not read or parse the file at all.
            let file_len = fs::metadata(&path)?.len();
            if file_len <= base_offset {
                // Still advance the index cursor so a freshly-rolled (but empty)
                // newer segment becomes the active one without re-`stat`ing the
                // old one next time.
                if Some(index) != self.last_index {
                    self.last_index = Some(index);
                    self.last_offset = 0;
                }
                continue;
            }

            // Read the whole (size-bounded) file, then frame-decode ONLY the
            // appended tail past the consumed offset.
            let data = fs::read(&path)?;
            let tail = data.get(base_offset as usize..).unwrap_or(&[]);
            let scan = segment::scan_bytes(tail);

            for entry in scan.entries {
                let dominated = self.last_rev.is_some_and(|lr| entry.rev <= lr);
                if !dominated {
                    new_entries.push(entry);
                }
            }

            // Advance the cursor: this is now the active segment, consumed up to
            // the clean boundary within the tail (base + tail's valid_len).
            self.last_index = Some(index);
            self.last_offset = base_offset.saturating_add(scan.valid_len);
        }

        if let Some(last) = new_entries.last() {
            self.last_rev = Some(last.rev);
        }
        Ok(new_entries)
    }

    /// The highest `rev` delivered so far, or `None` if no entries have been
    /// polled yet.
    #[must_use]
    pub const fn last_rev(&self) -> Option<u64> {
        self.last_rev
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_oplog::append::OplogWriter;
    use cp_wire::types::ContentHash;
    use cp_wire::types::Phase;
    use cp_wire::types::oplog::OpEntryKind;
    use tempfile::tempdir;

    fn phase_kind() -> OpEntryKind {
        OpEntryKind::PhaseTransition { phase: Phase::Streaming }
    }

    fn msg(thread: &str, byte: u8) -> OpEntryKind {
        OpEntryKind::MessageCreated {
            thread_id: thread.to_owned(),
            message_id: format!("m{byte}"),
            head: ContentHash::new([byte; 32]),
            inline_body: None,
        }
    }

    #[test]
    fn poll_returns_new_entries() {
        let dir = tempdir().expect("dir");
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        for byte in 0..4u8 {
            let _r = writer.append(msg("T1", byte)).expect("append");
        }

        let mut tailer = Tailer::new(dir.path().to_path_buf());
        let first = tailer.poll().expect("poll");
        assert_eq!(first.len(), 4, "first poll catches up the entire log");
        let revs: Vec<u64> = first.iter().map(|e| e.rev).collect();
        assert_eq!(revs, vec![0, 1, 2, 3]);
        assert_eq!(tailer.last_rev(), Some(3));
    }

    #[test]
    fn poll_is_idempotent_then_delivers_new() {
        let dir = tempdir().expect("dir");
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        let _r = writer.append(phase_kind()).expect("append");

        let mut tailer = Tailer::new(dir.path().to_path_buf());
        let _catch_up = tailer.poll().expect("poll");
        let empty = tailer.poll().expect("poll");
        assert!(empty.is_empty(), "no new writes → empty poll");

        let _r = writer.append(msg("T1", 0xAA)).expect("append");
        let new = tailer.poll().expect("poll");
        assert_eq!(new.len(), 1);
        assert_eq!(new.first().expect("entry").rev, 1);
    }

    #[test]
    fn poll_across_segment_roll_is_gap_free() {
        let dir = tempdir().expect("dir");
        let mut writer = OplogWriter::open_with_segment_limit(dir.path(), 16).expect("open");

        let mut tailer = Tailer::new(dir.path().to_path_buf());
        // Append enough to force several segment rolls.
        for byte in 0..10u8 {
            let _r = writer.append(msg("T1", byte)).expect("append");
        }
        let all = tailer.poll().expect("poll");
        // Revs must be strictly increasing and contain every user record +
        // every checkpoint the rolls injected. The exact count depends on
        // frame sizes, but monotonicity is the invariant.
        for window in all.windows(2) {
            assert!(
                window.get(1).expect("w1").rev > window.first().expect("w0").rev,
                "revs must strictly increase across segment rolls",
            );
        }
        assert!(all.len() >= 10, "at least 10 user records (plus checkpoints)");
    }

    #[test]
    fn seed_skips_known_history() {
        let dir = tempdir().expect("dir");
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        for byte in 0..5u8 {
            let _r = writer.append(msg("T1", byte)).expect("append");
        }

        let mut tailer = Tailer::new(dir.path().to_path_buf());
        tailer.seed(2); // skip revs 0, 1, 2
        let after_seed = tailer.poll().expect("poll");
        let revs: Vec<u64> = after_seed.iter().map(|e| e.rev).collect();
        assert_eq!(revs, vec![3, 4], "seed(2) skips revs 0-2");
    }

    #[test]
    fn poll_on_missing_dir_returns_empty() {
        let dir = tempdir().expect("dir");
        let mut tailer = Tailer::new(dir.path().join("nonexistent"));
        assert!(tailer.poll().expect("poll").is_empty());
    }
}
