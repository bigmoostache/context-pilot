//! Unit tests for [`crate::append`]: rev assignment, rolls, torn-tail
//! recovery, and the failure paths (poisoning, single-writer lock, re-stamping
//! an empty rolled segment, mid-log corruption).

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
    let revs: Vec<u64> = std::iter::repeat_with(|| writer.append(phase_kind()).expect("append")).take(10).collect();
    assert_eq!(revs, (0u64..10).collect::<Vec<_>>(), "revs strictly increase from 0");
}

#[test]
fn revs_resume_across_reopen_without_reuse() {
    let dir = tempdir().expect("tempdir");
    {
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        for _unused in 0u32..5 {
            let _rev = writer.append(phase_kind()).expect("append");
        }
        assert_eq!(writer.next_rev(), 5);
    }
    // Reopen: next rev must continue at 5, never reusing 0..4.
    let mut reopened = OplogWriter::open(dir.path()).expect("reopen");
    assert_eq!(reopened.next_rev(), 5, "rev resumes past the highest durable rev");
    let rev = reopened.append(phase_kind()).expect("append");
    assert_eq!(rev, 5);
}

#[test]
fn appended_records_read_back_in_order() {
    let dir = tempdir().expect("tempdir");
    let mut writer = OplogWriter::open(dir.path()).expect("open");
    for _unused in 0u32..4 {
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
    for _unused in 0u32..6 {
        last = writer.append(phase_kind()).expect("append");
    }
    assert!(writer.current_segment() > 0, "tiny limit must have rolled segments");
    let after_first_session = writer.next_rev();
    assert!(after_first_session > last, "next_rev is past the last appended rev");

    drop(writer); // release the single-writer lock

    // Reopen across multiple segments: rev resumes exactly where it stopped.
    let reopened = OplogWriter::open_with_segment_limit(dir.path(), 16).expect("reopen");
    assert_eq!(reopened.next_rev(), after_first_session, "rev resumes across multi-segment scan, checkpoints included");
}

#[test]
fn torn_tail_is_truncated_on_open() {
    let dir = tempdir().expect("tempdir");
    {
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        let _rev = writer.append(phase_kind()).expect("append");
        let _rev2 = writer.append(phase_kind()).expect("append");
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
            agent_id: "T1".to_owned(),
            message_id: "m1".to_owned(),
            head: cp_wire::types::ContentHash::new([0x77; 32]),
            inline_body: None,
        })
        .expect("append");
    let _cp = writer.checkpoint().expect("checkpoint");

    // The newest segment now ends with a checkpoint whose heads match the
    // running heads.
    let scan = segment::read(&segment::path(dir.path(), 0)).expect("read");
    // Own the last entry so the checkpoint pattern binds `snapshot` by value —
    // matching a `&OpEntryKind` via ergonomics trips pattern_type_mismatch, and
    // its mandated `ref` fix trips ref_patterns (the two are mutually exclusive).
    let last_kind = scan.entries.into_iter().next_back().expect("entries").kind;
    let OpEntryKind::Checkpoint { snapshot } = last_kind else {
        panic!("expected trailing checkpoint");
    };
    assert_eq!(&snapshot.heads, writer.heads());
}

#[test]
fn failed_write_poisons_and_acked_records_survive_reopen() {
    let dir = tempdir().expect("tempdir");
    let acked = {
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        let acked: Vec<u64> = std::iter::repeat_with(|| writer.append(phase_kind()).expect("append")).take(3).collect();

        writer.fail_next_write = true;
        assert!(writer.append(phase_kind()).is_err(), "injected write must fail");
        assert!(writer.is_poisoned());
        // Before the fix these succeeded, landing after the partial frame where
        // the next open's torn-tail scan would truncate them away.
        assert!(writer.append(phase_kind()).is_err(), "poisoned writer must refuse appends");
        assert!(writer.sync().is_err(), "poisoned writer must refuse sync");
        acked
    };

    let mut reopened = OplogWriter::open(dir.path()).expect("reopen truncates the partial frame");
    let state = replay::replay(dir.path()).expect("replay");
    assert_eq!(state.rev_head, acked.last().copied(), "every acknowledged rev survives");
    let next = reopened.append(phase_kind()).expect("append after reopen");
    assert_eq!(Some(next), acked.last().map(|rev| rev.wrapping_add(1)));
}

#[test]
fn second_writer_is_refused_until_first_drops() {
    let dir = tempdir().expect("tempdir");
    let first = OplogWriter::open(dir.path()).expect("open");
    assert!(OplogWriter::open(dir.path()).is_err(), "a second writer would interleave frames");
    drop(first);
    let _second = OplogWriter::open(dir.path()).expect("lock released on drop");
}

#[test]
fn empty_rolled_segment_is_restamped_on_open() {
    let dir = tempdir().expect("tempdir");
    {
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        let _rev = writer.append(phase_kind()).expect("append");
    }
    // Crash right after creating seg-1, before its checkpoint hit the disk.
    let _empty = File::create(segment::path(dir.path(), 1)).expect("create empty seg-1");

    let writer = OplogWriter::open(dir.path()).expect("reopen");
    assert_eq!(writer.current_segment(), 1);
    let first = segment::read_first(&segment::path(dir.path(), 1)).expect("read").expect("non-empty");
    assert!(matches!(first.kind, OpEntryKind::Checkpoint { .. }), "rolled segment must open with a checkpoint");
}

#[test]
fn corruption_in_an_older_segment_is_an_error_not_a_torn_tail() {
    let dir = tempdir().expect("tempdir");
    {
        let mut writer = OplogWriter::open_with_segment_limit(dir.path(), 16).expect("open");
        for _ in 0..6u8 {
            let _rev = writer.append(phase_kind()).expect("append");
        }
    }
    assert!(segment::indices(dir.path()).expect("indices").len() > 2, "tiny limit must have rolled");

    // Flip a payload byte in seg-0 (fully synced, never the newest).
    let seg0 = segment::path(dir.path(), 0);
    let mut bytes = fs::read(&seg0).expect("read seg-0");
    let last = bytes.last_mut().expect("non-empty");
    *last ^= 0xFF;
    fs::write(&seg0, &bytes).expect("corrupt seg-0");

    let indices = segment::indices(dir.path()).expect("indices");
    let err = replay::replay_full(dir.path(), &indices).expect_err("mid-log corruption must surface");
    assert!(err.to_string().contains("corrupt"), "unexpected error: {err}");
}
