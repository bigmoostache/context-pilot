//! Crash-replay harness — MILESTONE M0 (design doc Phase 9, V1/V2).
//!
//! The unit tests in [`crate::segment`] and [`crate::append`] prove torn-tail
//! recovery and exactly-once dedup against *synthetic* corruption (hand-flipped
//! bytes, in-process re-open). That is necessary but not sufficient: the
//! load-bearing claim of the whole design — **announce-after-durable survives a
//! real crash** (design doc K9) — can only be falsified by an *actual* process
//! that is `SIGKILL`'d and a *separate* process that recovers its log. This
//! harness is that forcing function.
//!
//! # What it validates
//!
//! * **V1 — torn tail discarded.** A child process appends durably in a tight
//!   loop and is `kill -9`'d at an arbitrary instant. A fresh process reopens
//!   the oplog: the reopen never errors, every `rev` the child announced as
//!   durable (post-`fdatasync`) is still replayable, and any half-written
//!   trailing frame left by the kill is silently truncated away.
//! * **V2 — journal-then-ack ⇒ exactly-once across a deadman re-exec.** A child
//!   journals a command effect (its `rev` becomes durable *before* the child
//!   announces it), then dies. A fresh process replays, finds the command's
//!   dedup token already in the seen-set, and a *duplicate* delivery of the same
//!   token folds as a no-op — the effect is applied exactly once even though the
//!   process that first journalled it is gone.
//!
//! V12 (the body-before-reference barrier) is intentionally **not** exercised
//! here: the content-addressed body store it guards does not exist until Phase
//! 14, so there is no spill path to crash mid-barrier yet. The decision rule
//! that resolves its GC race ([`cp_oplog::compact::body_gc_eligible`]) is
//! already unit-tested; the full crash-in-gap test lands with the body store.
//! See [`v12_barrier_is_pending_the_body_store`].
//!
//! # How the real crash is produced (std only, no extra deps)
//!
//! There is no separate child binary: the integration-test executable
//! re-invokes **itself** via [`std::env::current_exe`], running the single
//! env-gated entrypoint [`crash_child_entrypoint`]. The child and parent
//! synchronise through a **marker file** (not stdout, which the libtest harness
//! interleaves with its own framing): the child appends a line only *after* the
//! relevant `append` has returned, so a line in the marker is proof the `rev`
//! it names is durable. The parent polls the marker, then calls
//! [`std::process::Child::kill`] — which on Unix is `SIGKILL`, a true `kill -9`.
//!
//! The harness is Unix-only (it relies on `SIGKILL` semantics); on other
//! platforms it compiles to nothing.

#![cfg(unix)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cp_oplog::append::OplogWriter;
use cp_oplog::replay::replay;
use cp_wire::types::oplog::OpEntryKind;

/// Environment variable carrying the oplog directory to the re-exec'd child.
/// Its presence is what flips [`crash_child_entrypoint`] from a no-op into the
/// child workload.
const CHILD_DIR_ENV: &str = "CP_CRASH_CHILD_DIR";

/// Environment variable selecting which child workload to run (`spam` or
/// `command`).
const CHILD_MODE_ENV: &str = "CP_CRASH_CHILD_MODE";

/// Marker file the child writes durability proofs into, relative to the oplog
/// directory.
const MARKER: &str = "marker.txt";

/// The dedup token the `command` child journals — the V2 subject.
const COMMAND_TOKEN: &str = "deadman-command-token";

// ── child entrypoint ─────────────────────────────────────────────────────

/// The re-exec'd child workload, gated on [`CHILD_DIR_ENV`].
///
/// In a normal `cargo test` run the variable is absent and this is a no-op that
/// passes instantly. When the parent re-invokes the test binary with the
/// variable set, this performs the crash-victim workload for the selected mode
/// and then spins until the parent `SIGKILL`s it. It must never return in child
/// mode (a clean exit would defeat the "killed mid-flight" premise).
#[test]
fn crash_child_entrypoint() {
    let Ok(dir_var) = env::var(CHILD_DIR_ENV) else {
        return; // Parent role: nothing to do here.
    };
    let dir = PathBuf::from(dir_var);
    let mode = env::var(CHILD_MODE_ENV).unwrap_or_else(|_unset| "spam".to_owned());

    match mode.as_str() {
        "command" => child_command(&dir),
        _ => child_spam(&dir),
    }
}

/// V1 child: append durable phase records forever, recording each durable
/// `rev` to the marker only *after* `append` (and its `fdatasync`) returned.
fn child_spam(dir: &Path) -> ! {
    let mut writer = OplogWriter::open(dir).unwrap_or_else(|e| panic!("child open: {e}"));
    let mut byte: u8 = 0;
    loop {
        let kind = OpEntryKind::MessageCreated {
            thread_id: "T1".to_owned(),
            message_id: format!("m{byte}"),
            head: cp_wire::types::ContentHash::new([byte; 32]),
            inline_body: None,
        };
        let rev = writer.append(kind).unwrap_or_else(|e| panic!("child append: {e}"));
        append_marker(dir, &format!("DURABLE {rev}"));
        byte = byte.wrapping_add(1);
    }
}

/// V2 child: journal exactly one command effect, prove it durable via the
/// marker, then spin so the parent can `SIGKILL` the journalling process.
fn child_command(dir: &Path) -> ! {
    let mut writer = OplogWriter::open(dir).unwrap_or_else(|e| panic!("child open: {e}"));
    let kind = OpEntryKind::CommandEffect { cmd_id: "deadman-cmd".to_owned(), dedup_token: COMMAND_TOKEN.to_owned() };
    let rev = writer.append(kind).unwrap_or_else(|e| panic!("child command append: {e}"));
    append_marker(dir, &format!("ACKED {rev}"));
    loop {
        sleep(Duration::from_millis(50));
    }
}

/// Append one line to the marker file, flushing it durably so the parent always
/// observes a line whose `rev` is genuinely on disk.
fn append_marker(dir: &Path, line: &str) {
    use std::io::Write as _;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(MARKER))
        .unwrap_or_else(|e| panic!("child marker open: {e}"));
    writeln!(file, "{line}").unwrap_or_else(|e| panic!("child marker write: {e}"));
    file.sync_data().unwrap_or_else(|e| panic!("child marker sync: {e}"));
}

// ── parent helpers ───────────────────────────────────────────────────────

/// Spawn the test binary as a child running [`crash_child_entrypoint`] against
/// `dir` in `mode`.
fn spawn_child(dir: &Path, mode: &str) -> std::process::Child {
    let exe = env::current_exe().unwrap_or_else(|e| panic!("current_exe: {e}"));
    Command::new(exe)
        .args(["--exact", "crash_child_entrypoint", "--nocapture"])
        .env(CHILD_DIR_ENV, dir)
        .env(CHILD_MODE_ENV, mode)
        .spawn()
        .unwrap_or_else(|e| panic!("spawn child: {e}"))
}

/// Read every line of the marker file, or an empty list if it does not exist
/// yet.
fn read_marker(dir: &Path) -> Vec<String> {
    match fs::read_to_string(dir.join(MARKER)) {
        Ok(text) => text.lines().map(str::to_owned).collect(),
        Err(_absent) => Vec::new(),
    }
}

/// Block until the marker holds at least `n` lines, or panic after a timeout so
/// a wedged child fails the test loudly instead of hanging CI.
fn wait_for_marker_lines(dir: &Path, n: usize) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let lines = read_marker(dir);
        if lines.len() >= n {
            return lines;
        }
        if Instant::now() > deadline {
            panic!("child never produced {n} marker lines (got {})", lines.len());
        }
        sleep(Duration::from_millis(5));
    }
}

/// Parse the trailing `rev` out of a marker line like `DURABLE 7` / `ACKED 3`.
fn rev_of(line: &str) -> u64 {
    line.rsplit(' ')
        .next()
        .and_then(|tok| tok.parse::<u64>().ok())
        .unwrap_or_else(|| panic!("malformed marker line: {line:?}"))
}

// ── V1 — torn tail discarded under a real SIGKILL ─────────────────────────

#[test]
fn v1_real_sigkill_leaves_a_clean_replayable_oplog() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path();

    let mut child = spawn_child(path, "spam");
    // Let the child commit a healthy run of durable records before we kill it.
    let lines = wait_for_marker_lines(path, 8);
    let highest_announced = lines.iter().map(|l| rev_of(l)).max().expect("a durable rev");

    // A true kill -9, mid-flight: the child may be anywhere — between write and
    // fdatasync, mid write_all, or idle. Whatever the instant, reopen must cope.
    child.kill().expect("SIGKILL child");
    let _status = child.wait().expect("reap child");

    // A fresh process recovers the log left by the dead one.
    let state = replay(path).expect("replay after SIGKILL must not error");

    let rev_head = state.rev_head.expect("recovered log is non-empty");
    // Every rev the child *announced* as durable (post-sync) must survive — that
    // is announce-after-durable (K9) holding across a real crash.
    assert!(
        rev_head >= highest_announced,
        "announced-durable rev {highest_announced} lost after crash (rev_head={rev_head})",
    );

    // Re-opening the writer must also succeed: it truncates any torn tail the
    // kill left and resumes at a clean boundary (V1), never reusing a rev.
    let writer = OplogWriter::open(path).expect("reopen writer after crash");
    assert!(
        writer.next_rev() > rev_head,
        "next_rev {} must advance past the recovered head {rev_head}",
        writer.next_rev(),
    );
}

// ── V2 — exactly-once across a deadman re-exec ────────────────────────────

#[test]
fn v2_journalled_command_is_exactly_once_after_deadman_reexec() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path();

    // Child journals the command (durable) then spins; we kill the journalling
    // process so recovery happens in a genuinely separate process.
    let mut child = spawn_child(path, "command");
    let lines = wait_for_marker_lines(path, 1);
    let acked_line = lines.first().expect("acked line");
    assert!(acked_line.starts_with("ACKED "), "expected an ACKED marker, got {acked_line:?}");
    let acked_rev = rev_of(acked_line);

    child.kill().expect("SIGKILL child");
    let _status = child.wait().expect("reap child");

    // Deadman re-exec: a fresh process replays the dead one's oplog.
    let recovered = replay(path).expect("replay after deadman kill");
    assert_eq!(recovered.rev_head, Some(acked_rev), "the journalled command survived the crash");
    assert!(
        recovered.seen.contains(COMMAND_TOKEN),
        "journal-then-ack means the acked command's token is in the recovered seen-set",
    );

    // A duplicate delivery of the very same command (its dedup token) must fold
    // to a no-op — exactly-once, even across the process boundary.
    let seen_before = recovered.seen.len();
    let mut writer = OplogWriter::open(path).expect("reopen writer");
    let _dup_rev = writer
        .append(OpEntryKind::SeenMark { dedup_token: COMMAND_TOKEN.to_owned() })
        .expect("append duplicate delivery");

    let after = replay(path).expect("replay after duplicate delivery");
    assert_eq!(
        after.seen.len(),
        seen_before,
        "a duplicate delivery of an already-seen token must not grow the seen-set",
    );
    assert!(after.seen.contains(COMMAND_TOKEN), "the token remains seen");
}

// ── V12 — pending the body store (Phase 14) ───────────────────────────────

/// V12 (no durable entry ever references a missing body) cannot be crash-tested
/// until the content-addressed body store and its spill path exist (Phase 14):
/// there is no barrier with two sides to crash between yet. The GC *decision
/// rule* that makes a crash-orphan distinguishable from an in-flight spill is
/// already in place and unit-tested in [`cp_oplog::compact`]; this test asserts
/// the rule here too, and stands as the explicit placeholder for the full
/// crash-in-gap test that lands with the body store.
#[test]
fn v12_barrier_is_pending_the_body_store() {
    use cp_oplog::compact::{DEFAULT_GC_GRACE, body_gc_eligible};
    // An in-flight spill (young) is never collected; a provable crash-orphan
    // (older than any barrier window) is.
    assert!(!body_gc_eligible(Duration::from_secs(1), DEFAULT_GC_GRACE));
    assert!(body_gc_eligible(DEFAULT_GC_GRACE + Duration::from_secs(1), DEFAULT_GC_GRACE));
}
