//! Oplog replay — rebuild an agent's current heads + `rev` from the log.
//!
//! Replay is how the agent (after a deadman re-exec) and the backend (on
//! restart) recover authoritative state: fold the durable [`OpEntry`] stream
//! into a bounded [`Heads`] snapshot (design doc I3) and the highest durable
//! `rev`. A torn tail left by an interrupted write is already discarded by
//! [`segment::read`], so replay only ever sees clean records (design doc V1).
//!
//! # Bounded replay — the head-checkpoint mechanism (design doc GAP 1 / I5)
//!
//! Rebuilding heads by folding *every* record from offset 0 would be
//! O(total-oplog) — disk-bound, contradicting I5 ("restart latency bounded by
//! agent **count**, not fleet **disk**"). To bound it, the writer emits a
//! **checkpoint** — a full [`Heads`] snapshot — as the **first record of every
//! rolled segment** ([`crate::append::OplogWriter`]). Replay then needs to read
//! only the **newest** segment: its leading checkpoint is the base, and the
//! records after it are the only fold work. Replay cost is therefore bounded by
//! one segment (≤ the segment size limit), independent of total oplog size.
//!
//! The first segment (`seg-0`) carries no leading checkpoint (there is no prior
//! state to snapshot), so a young, single-segment oplog has no fast-path
//! checkpoint; [`replay`] then falls back to a full fold — which is still
//! bounded, because a single segment is itself size-capped. Once the log has
//! rolled even once, every newer segment is self-sufficient and the fast path
//! applies.

use cp_wire::types::oplog::{OpEntry, OpEntryKind};
use cp_wire::types::snapshot::{Heads, RosterThread, SeenSet};

use crate::error::OplogResult;
use crate::segment;
use std::path::Path;

/// The recovered state of an oplog: its highest durable `rev` and the bounded
/// snapshot (heads + seen-set + roster) as of that `rev`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recovered {
    /// The highest durable `rev` in the log, or `None` if the log is empty.
    pub rev_head: Option<u64>,

    /// The bounded head set (per-thread last-message hash, per-panel hash) as
    /// of `rev_head`.
    pub heads: Heads,

    /// The dedup-tokens of durable command effects as of `rev_head` (I4).
    pub seen: SeenSet,

    /// The thread roster as of `rev_head` — one entry per live thread, folded
    /// from the thread-roster deltas so the writer can stamp it into each
    /// segment's leading checkpoint (design doc I5 / §16). The agent does not
    /// read this for its own recovery (it reloads threads from tier-② state);
    /// it exists so the **backend** can rebuild the roster from a checkpoint
    /// after oplog compaction, exactly as it rebuilds heads.
    pub roster: Vec<RosterThread>,
}

/// Fold one entry into a running [`Recovered`].
///
/// A `Checkpoint` is **authoritative**: it replaces the running snapshot
/// wholesale (heads, seen-set, and roster — it is a full snapshot, not a
/// delta). A `MessageCreated` advances its thread's head and bumps that
/// thread's roster activity. The thread-roster deltas
/// (`ThreadCreated`/`Archived`/`Restored`/`StatusChanged`) maintain the roster
/// idempotently. A `CommandEffect` or `SeenMark` marks its dedup token as seen
/// at this entry's `rev` (I4). The remaining variants carry no recoverable
/// state and are no-ops here.
///
/// The `rev` is taken from `entry`, so the seen-set records the exact `rev` at
/// which each command's effect first committed.
pub(crate) fn fold_entry(state: &mut Recovered, entry: &OpEntry) {
    match &entry.kind {
        OpEntryKind::Checkpoint { snapshot } => {
            state.heads.clone_from(&snapshot.heads);
            state.seen.clone_from(&snapshot.seen);
            state.roster.clone_from(&snapshot.roster);
        }
        OpEntryKind::MessageCreated { thread_id, head, .. } => {
            state.heads.set_thread_head(thread_id, *head);
            RosterThread::fold_message(&mut state.roster, thread_id, entry.timestamp_ms);
        }
        OpEntryKind::CommandEffect { dedup_token, .. } | OpEntryKind::SeenMark { dedup_token } => {
            state.seen.mark(dedup_token, entry.rev);
        }
        OpEntryKind::ThreadCreated { thread_id, name, status, timestamp_ms } => {
            RosterThread::fold_created(
                &mut state.roster,
                cp_wire::types::snapshot::ThreadCreation {
                    thread_id,
                    name,
                    status: *status,
                    timestamp_ms: *timestamp_ms,
                },
            );
        }
        OpEntryKind::ThreadArchived { thread_id } => {
            RosterThread::fold_archived(&mut state.roster, thread_id, true);
        }
        OpEntryKind::ThreadRestored { thread_id } => {
            RosterThread::fold_archived(&mut state.roster, thread_id, false);
        }
        OpEntryKind::ThreadStatusChanged { thread_id, status } => {
            RosterThread::fold_status(&mut state.roster, thread_id, *status);
        }
        // Phase, lifecycle, and cost carry no head/seen/roster state; an
        // `Unknown` variant from a newer schema is ignored (forward-compat).
        OpEntryKind::PhaseTransition { .. }
        | OpEntryKind::Lifecycle { .. }
        | OpEntryKind::CostAggregate { .. }
        | OpEntryKind::Unknown => {}
    }
}

/// Replay the oplog in `dir`, returning its highest durable `rev` and heads.
///
/// Takes the fast path when the newest non-empty segment begins with a
/// checkpoint (the common case once the log has rolled), reading only that one
/// segment. Otherwise — a young single-segment log, or a segment whose leading
/// checkpoint was torn away — it falls back to a full fold from the oldest
/// segment, which is still bounded for the single-segment case.
///
/// # Errors
///
/// Returns [`Error::Io`](crate::error::Error::Io) if a segment
/// cannot be listed or read.
pub fn replay<P: AsRef<Path>>(dir: P) -> OplogResult<Recovered> {
    let dir = dir.as_ref();
    let indices = segment::indices(dir)?;

    if let Some(state) = replay_fast(dir, &indices)? {
        return Ok(state);
    }
    replay_full(dir, &indices)
}

/// Fast path: if the newest non-empty segment opens with a checkpoint, seed
/// from it and fold only that segment's trailing records. Returns `None` when
/// no segment offers a usable leading checkpoint (caller falls back to a full
/// fold).
fn replay_fast(dir: &Path, indices: &[u64]) -> OplogResult<Option<Recovered>> {
    for &index in indices.iter().rev() {
        let scan = segment::read(&segment::path(dir, index))?;
        let Some(first) = scan.entries.first() else {
            // Empty or torn-at-zero segment: try the next-older one.
            continue;
        };
        if let OpEntryKind::Checkpoint { snapshot } = &first.kind {
            let mut state = Recovered {
                rev_head: Some(first.rev),
                heads: snapshot.heads.clone(),
                seen: snapshot.seen.clone(),
                roster: snapshot.roster.clone(),
            };
            for entry in scan.entries.iter().skip(1) {
                fold_entry(&mut state, entry);
                state.rev_head = Some(entry.rev);
            }
            return Ok(Some(state));
        }
        // Newest non-empty segment has records but no leading checkpoint
        // (only seg-0, by construction): the full fold handles it correctly.
        return Ok(None);
    }
    Ok(None)
}

/// Slow path: fold every record across every segment, oldest to newest. Always
/// correct; used only when the fast path declines (young single-segment log or
/// an invariant-violating segment).
fn replay_full(dir: &Path, indices: &[u64]) -> OplogResult<Recovered> {
    let mut state = Recovered::default();
    for &index in indices {
        let scan = segment::read(&segment::path(dir, index))?;
        for entry in &scan.entries {
            fold_entry(&mut state, entry);
            state.rev_head = Some(entry.rev);
        }
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::append::OplogWriter;
    use cp_wire::types::{ContentHash, Phase};
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
    fn empty_oplog_replays_to_default() {
        let dir = tempdir().expect("tempdir");
        let _writer = OplogWriter::open(dir.path()).expect("open");
        let state = replay(dir.path()).expect("replay");
        assert_eq!(state.rev_head, None);
        assert_eq!(state.heads, Heads::default());
    }

    #[test]
    fn replay_rebuilds_latest_thread_heads() {
        let dir = tempdir().expect("tempdir");
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        let _a = writer.append(msg("T1", 0x11)).expect("append");
        let _b = writer.append(msg("T2", 0x22)).expect("append");
        let last = writer.append(msg("T1", 0x33)).expect("append"); // overwrites T1

        let state = replay(dir.path()).expect("replay");
        assert_eq!(state.rev_head, Some(last));
        // Heads equality is order-sensitive; build the expected set in the same
        // insertion order the fold produces (T1 first — inserted then updated
        // in place — then T2).
        let mut expected = Heads::default();
        expected.set_thread_head("T1", ContentHash::new([0x33; 32]));
        expected.set_thread_head("T2", ContentHash::new([0x22; 32]));
        assert_eq!(state.heads, expected);
    }

    #[test]
    fn replay_ignores_non_head_entries() {
        let dir = tempdir().expect("tempdir");
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        let _p = writer.append(OpEntryKind::PhaseTransition { phase: Phase::Streaming }).expect("a");
        let last = writer.append(msg("T1", 0x44)).expect("append");

        let state = replay(dir.path()).expect("replay");
        assert_eq!(state.rev_head, Some(last));
        let mut expected = Heads::default();
        expected.set_thread_head("T1", ContentHash::new([0x44; 32]));
        assert_eq!(state.heads, expected);
    }

    #[test]
    fn fast_path_matches_full_path_across_segments() {
        let dir = tempdir().expect("tempdir");
        // Tiny limit forces rolls, so segments past seg-0 carry leading
        // checkpoints and the fast path engages.
        let mut writer = OplogWriter::open_with_segment_limit(dir.path(), 16).expect("open");
        for byte in 0..12u8 {
            let _r = writer.append(msg("T1", byte)).expect("append");
        }
        let extra = writer.append(msg("T2", 0xEE)).expect("append");

        let fast = replay(dir.path()).expect("fast");
        let indices = segment::indices(dir.path()).expect("indices");
        let full = replay_full(dir.path(), &indices).expect("full");
        assert_eq!(fast, full, "fast path must equal full replay");
        assert_eq!(fast.rev_head, Some(extra));
        let mut expected = Heads::default();
        expected.set_thread_head("T1", ContentHash::new([11; 32]));
        expected.set_thread_head("T2", ContentHash::new([0xEE; 32]));
        assert_eq!(fast.heads, expected);
    }

    #[test]
    fn rolled_segments_open_with_a_checkpoint() {
        let dir = tempdir().expect("tempdir");
        let mut writer = OplogWriter::open_with_segment_limit(dir.path(), 16).expect("open");
        for byte in 0..8u8 {
            let _r = writer.append(msg("T1", byte)).expect("append");
        }
        let indices = segment::indices(dir.path()).expect("indices");
        assert!(indices.len() > 1, "tiny limit must have rolled");
        // Every segment after seg-0 must begin with a checkpoint (the bounded
        // replay invariant).
        for &index in indices.iter().skip(1) {
            let scan = segment::read(&segment::path(dir.path(), index)).expect("read");
            let first = scan.entries.first().expect("non-empty rolled segment");
            assert!(
                matches!(first.kind, OpEntryKind::Checkpoint { .. }),
                "rolled segment {index} must open with a checkpoint",
            );
        }
    }

    #[test]
    fn replay_survives_reopen() {
        let dir = tempdir().expect("tempdir");
        {
            let mut writer = OplogWriter::open(dir.path()).expect("open");
            let _r = writer.append(msg("T1", 0x55)).expect("append");
        }
        // Reopen, append more; replay reflects both sessions.
        let mut writer = OplogWriter::open(dir.path()).expect("reopen");
        let last = writer.append(msg("T1", 0x66)).expect("append");
        let state = replay(dir.path()).expect("replay");
        assert_eq!(state.rev_head, Some(last));
        let mut expected = Heads::default();
        expected.set_thread_head("T1", ContentHash::new([0x66; 32]));
        assert_eq!(state.heads, expected);
    }

    #[test]
    fn replay_rebuilds_seen_set_from_command_effects() {
        let dir = tempdir().expect("tempdir");
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        let _a = writer
            .append(OpEntryKind::CommandEffect {
                cmd_id: "c1".to_owned(),
                dedup_token: "tok-a".to_owned(),
            })
            .expect("append");
        // A duplicate delivery of the same token folds as a no-op.
        let _b = writer.append(OpEntryKind::SeenMark { dedup_token: "tok-a".to_owned() }).expect("dup");
        let _c = writer
            .append(OpEntryKind::CommandEffect {
                cmd_id: "c2".to_owned(),
                dedup_token: "tok-b".to_owned(),
            })
            .expect("append");

        let state = replay(dir.path()).expect("replay");
        assert!(state.seen.contains("tok-a"), "first command's token is seen");
        assert!(state.seen.contains("tok-b"), "second command's token is seen");
        assert_eq!(state.seen.len(), 2, "duplicate delivery added no extra entry");
    }

    #[test]
    fn replay_rebuilds_roster_from_thread_deltas() {
        let dir = tempdir().expect("tempdir");
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        let _c = writer
            .append(OpEntryKind::ThreadCreated {
                thread_id: "T1".to_owned(),
                name: "Plan".to_owned(),
                status: cp_wire::types::ThreadTurn::TheirTurn,
                timestamp_ms: 100,
            })
            .expect("create");
        let _m = writer.append(msg("T1", 0x01)).expect("message");
        let _a = writer.append(OpEntryKind::ThreadArchived { thread_id: "T1".to_owned() }).expect("archive");

        let state = replay(dir.path()).expect("replay");
        assert_eq!(state.roster.len(), 1);
        let e = state.roster.first().expect("entry");
        assert_eq!(e.thread_id, "T1");
        assert_eq!(e.name, "Plan");
        assert_eq!(e.msg_count, 1, "the message bumped the count");
        assert!(e.archived, "the archive delta folded");
    }

    #[test]
    fn roster_survives_compaction_via_checkpoint() {
        // The roster, like heads and the seen-set, must be recoverable by
        // reading only the newest checkpoint-bearing segment after rolls — the
        // backend's cold-restart-after-compaction guarantee (I5).
        let dir = tempdir().expect("tempdir");
        let mut writer = OplogWriter::open_with_segment_limit(dir.path(), 16).expect("open");
        let _c = writer
            .append(OpEntryKind::ThreadCreated {
                thread_id: "T-early".to_owned(),
                name: "Early".to_owned(),
                status: cp_wire::types::ThreadTurn::MyTurn,
                timestamp_ms: 1,
            })
            .expect("create");
        // Force several rolls so the roster is carried only by a checkpoint.
        for byte in 0..8u8 {
            let _r = writer.append(msg("T-other", byte)).expect("append");
        }
        let indices = segment::indices(dir.path()).expect("indices");
        assert!(indices.len() > 1, "tiny limit must have rolled");

        let fast = replay(dir.path()).expect("fast");
        let full = replay_full(dir.path(), &indices).expect("full");
        assert_eq!(fast, full, "fast path (checkpoint-seeded) equals full replay");
        assert!(
            fast.roster.iter().any(|e| e.thread_id == "T-early"),
            "the early thread survives via the checkpoint roster",
        );
    }

    #[test]
    fn seen_set_survives_checkpoint_and_reopen() {
        // V3 across the real replay path: a token committed before a segment
        // roll lands in the rolled segment's leading checkpoint, so replay
        // recovers it by reading only the newest segment — no time bound, no
        // dependence on folding from offset 0.
        let dir = tempdir().expect("tempdir");
        let mut writer = OplogWriter::open_with_segment_limit(dir.path(), 16).expect("open");
        let _e = writer
            .append(OpEntryKind::CommandEffect {
                cmd_id: "c1".to_owned(),
                dedup_token: "tok-early".to_owned(),
            })
            .expect("append");
        // Force several rolls so the token is carried only by a checkpoint.
        for byte in 0..8u8 {
            let _r = writer.append(msg("T1", byte)).expect("append");
        }
        let indices = segment::indices(dir.path()).expect("indices");
        assert!(indices.len() > 1, "tiny limit must have rolled");

        let fast = replay(dir.path()).expect("fast");
        let full = replay_full(dir.path(), &indices).expect("full");
        assert_eq!(fast, full, "fast path (checkpoint-seeded) equals full replay");
        assert!(fast.seen.contains("tok-early"), "token survives via checkpoint");
    }
}
