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
        Self { dir: oplog_dir, last_index: None, last_rev: None }
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
    /// # Errors
    ///
    /// Returns [`io::Error`] if a segment file cannot be listed or read.
    pub fn poll(&mut self) -> io::Result<Vec<OpEntry>> {
        let indices = segment::indices(&self.dir)?;
        let start_from = self.last_index.unwrap_or(0);
        let mut new_entries: Vec<OpEntry> = Vec::new();

        for &index in &indices {
            if index < start_from {
                continue;
            }
            let scan = segment::read(&segment::path(&self.dir, index))
                .map_err(|e| io::Error::other(e.to_string()))?;
            for entry in scan.entries {
                let dominated = self.last_rev.is_some_and(|lr| entry.rev <= lr);
                if !dominated {
                    new_entries.push(entry);
                }
            }
        }

        if let Some(last) = new_entries.last() {
            self.last_rev = Some(last.rev);
        }
        if let Some(&newest_index) = indices.last() {
            self.last_index = Some(newest_index);
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
    use cp_wire::types::oplog::OpEntryKind;
    use cp_wire::types::ContentHash;
    use cp_wire::types::Phase;
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
