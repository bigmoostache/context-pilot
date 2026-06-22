//! Phase 28 — the backend [`AgentSupervisor`] driven against **real OS
//! processes**, through its public API only.
//!
//! The in-crate unit tests (`supervisor/tests.rs`) prove each method against a
//! cooperative `/bin/sleep` and the `pub(crate)` allow-list gate. This suite
//! proves the parts that only real process semantics exercise, and that the
//! unit tests cannot reach black-box:
//!
//! * **SIGTERM → grace → SIGKILL escalation** against an agent that *ignores*
//!   SIGTERM — the unit `spawn_and_stop` uses a process that dies on SIGTERM
//!   immediately and so never walks the escalation path.
//! * **Fleet teardown** — many agents supervised and torn down together, each
//!   process provably gone from the OS afterward.
//! * **A foreign (adopted) process is really signalled on stop** — `adopt`
//!   records a process the backend did not spawn, and `stop` must still drive a
//!   signal to it.
//! * **`check_liveness` routes a spawned exit and an adopted vanish in one
//!   pass** — `try_wait` (spawned) and `kill(pid, 0)` (adopted) resolving
//!   simultaneously, the diff emptying the supervisor.

// Linked into this integration-test target but not named directly; acknowledge
// them for the per-target `unused-crate-dependencies` lint.
use cp_mod_bridge as _;
use cp_oplog as _;
use notify as _;
use portable_pty as _;
use serde as _;
use serde_json as _;
use serde_yaml as _;
use tiny_http as _;

use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;

use cp_orchestrator::supervisor::{AgentSupervisor, Event};
use cp_wire::types::registry::{AgentStatus, Entry};

use tempfile::tempdir;

/// `SIGTERM`'s signal number on every Unix the supervisor targets.
const SIGTERM_NUM: i32 = 15;

/// The supervisor's own SIGTERM→SIGKILL grace, plus margin for the poll loop.
/// Mirrors `supervisor::STOP_GRACE` (5s); tests that walk the escalation path
/// wait at least this long.
const ESCALATION_BUDGET: Duration = Duration::from_secs(8);

/// `true` while `pid` names a live (or zombie) process; `false` once the kernel
/// has no such pid (`ESRCH`).
fn pid_present(pid: u32) -> bool {
    let raw = Pid::from_raw(i32::try_from(pid).unwrap_or(i32::MAX));
    !matches!(kill(raw, None), Err(Errno::ESRCH))
}

/// A registry [`Entry`] naming an already-running `pid` in `folder`, the shape
/// [`AgentSupervisor::adopt`] consumes.
fn adopt_entry(id: &str, pid: u32, folder: &Path) -> Entry {
    Entry {
        schema_version: 1,
        id: id.to_owned(),
        folder: folder.to_string_lossy().into_owned(),
        pid,
        boot_id: "boot-adopt".to_owned(),
        model: "test-model".to_owned(),
        protocol_version: 1,
        binary_version: "0.0.0".to_owned(),
        socket_path: String::new(),
        oplog_path: String::new(),
        heartbeat_path: String::new(),
        cap_token: String::new(),
        started_at_ms: 0,
        status: AgentStatus::Running,
    }
}

// ── 1. SIGTERM is escalated to SIGKILL for a stubborn agent ─────────────────

#[test]
fn a_stubborn_agent_that_ignores_sigterm_is_killed_by_escalation() {
    let folder = tempdir().expect("folder");
    let mut sup = AgentSupervisor::new(&[PathBuf::from("/bin/sh")]);

    // A shell that traps (ignores) SIGTERM and then sleeps: stop() must fall
    // through the grace window and escalate to the uncatchable SIGKILL.
    let pid = sup
        .spawn("stubborn".to_owned(), Path::new("/bin/sh"), folder.path(), &["-c", "trap '' TERM; exec sleep 60"])
        .expect("spawn stubborn agent");
    assert!(pid_present(pid), "the agent is running");

    let start = Instant::now();
    sup.stop("stubborn").expect("stop");
    let elapsed = start.elapsed();

    assert!(sup.is_empty(), "the supervisor dropped the stopped agent");
    assert!(!pid_present(pid), "SIGKILL escalation removed the stubborn process");
    assert!(elapsed < ESCALATION_BUDGET, "stop completed within the grace+kill budget (took {elapsed:?})",);
}

// ── 2. a supervised fleet is tracked and fully torn down ────────────────────

#[test]
fn the_supervisor_tracks_and_tears_down_a_multi_agent_fleet() {
    let folder = tempdir().expect("folder");
    let mut sup = AgentSupervisor::new(&[PathBuf::from("/bin/sleep")]);

    let mut pids = Vec::new();
    for n in 0..3 {
        let pid = sup
            .spawn(format!("agent-{n}"), Path::new("/bin/sleep"), folder.path(), &["60"])
            .unwrap_or_else(|e| panic!("spawn agent-{n}: {e}"));
        pids.push(pid);
    }
    assert_eq!(sup.len(), 3, "three agents are supervised");
    assert!(pids.iter().all(|&p| pid_present(p)), "all three are running");

    for n in 0..3 {
        sup.stop(&format!("agent-{n}")).unwrap_or_else(|e| panic!("stop agent-{n}: {e}"));
    }
    assert!(sup.is_empty(), "the fleet is fully torn down");

    // Each spawned child was waited on by stop(), so none lingers as a zombie:
    // the kernel reports no such pid.
    thread::sleep(Duration::from_millis(100));
    assert!(pids.iter().all(|&p| !pid_present(p)), "every process is reaped and gone");
}

// ── 3. an adopted foreign process is really signalled on stop ───────────────

#[test]
fn an_adopted_foreign_process_is_signalled_on_stop() {
    let folder = tempdir().expect("folder");
    let mut sup = AgentSupervisor::new(&[]);

    // A process the supervisor did NOT spawn — the test owns the handle.
    let mut foreign = Command::new("/bin/sleep")
        .arg("60")
        .current_dir(folder.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn foreign process");
    let pid = foreign.id();

    sup.adopt("adopted".to_owned(), &adopt_entry("adopted", pid, folder.path()), PathBuf::from("/bin/sleep"))
        .expect("adopt");
    assert_eq!(sup.len(), 1, "the foreign process is adopted");

    // stop() signals the foreign pid; because the supervisor never spawned it,
    // it does not reap it — the test (its real parent) does, and observes the
    // termination signal.
    sup.stop("adopted").expect("stop adopted");
    assert!(sup.is_empty(), "the adopted agent is dropped");

    let status = foreign.wait().expect("reap foreign process");
    assert_eq!(
        status.signal(),
        Some(SIGTERM_NUM),
        "the adopted process was terminated by the SIGTERM stop() delivered",
    );
}

// ── 4. check_liveness routes a spawned exit and an adopted vanish together ──

#[test]
fn check_liveness_routes_a_spawned_exit_and_an_adopted_vanish_in_one_pass() {
    let folder = tempdir().expect("folder");
    let mut sup = AgentSupervisor::new(&[PathBuf::from("/bin/sleep")]);

    // A spawned agent that exits on its own almost immediately.
    let _spawned =
        sup.spawn("exits".to_owned(), Path::new("/bin/sleep"), folder.path(), &["0"]).expect("spawn exiting agent");

    // A foreign process we adopt, then let fully die + be reaped, so its pid is
    // genuinely gone before the liveness probe (an unreaped zombie would still
    // answer signal-0 as alive).
    let mut foreign = Command::new("/bin/sleep")
        .arg("0")
        .current_dir(folder.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn foreign");
    let gone_pid = foreign.id();
    let _status = foreign.wait().expect("reap foreign");

    sup.adopt("vanished".to_owned(), &adopt_entry("vanished", gone_pid, folder.path()), PathBuf::from("/bin/sleep"))
        .expect("adopt");
    assert_eq!(sup.len(), 2, "one spawned + one adopted are supervised");

    // Give the spawned `sleep 0` a moment to exit.
    thread::sleep(Duration::from_millis(250));

    let events = sup.check_liveness();
    assert_eq!(events.len(), 2, "both deaths are reported in one pass");

    let saw_exited = events.iter().any(|e| matches!(e, Event::Exited { agent_id, .. } if agent_id == "exits"));
    let saw_vanished = events.iter().any(|e| matches!(e, Event::Vanished { agent_id } if agent_id == "vanished"));
    assert!(saw_exited, "the spawned child is reported Exited (reaped via try_wait)");
    assert!(saw_vanished, "the adopted process is reported Vanished (signal-0 probe)");
    assert!(sup.is_empty(), "both dead agents are removed");
}
