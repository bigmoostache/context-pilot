//! Managed child sessions: spawn into a private process group, record the exit
//! status from a dedicated waiter thread, and terminate the whole process tree.
//!
//! The waiter thread is the ONLY writer of a session's exit status. Nothing else
//! probes the PID: once the waiter has reaped the child, the PID can be recycled
//! by the OS, and a `kill -0` / `kill -9` aimed at it could hit a stranger.

use std::io::Write as _;
use std::os::unix::process::{CommandExt as _, ExitStatusExt as _};
use std::process::{ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;

/// Exit status shared between a session and its waiter thread.
/// `None` while running, `Some(code)` once reaped.
pub(crate) type ExitCell = Arc<Mutex<Option<i32>>>;

/// How long `send` waits for the child to drain its stdin before giving up.
/// Below the client's 5 s socket read timeout so the reply still arrives.
const SEND_TIMEOUT: Duration = Duration::from_secs(3);

/// State of a single managed child process.
pub(crate) struct Session {
    /// PID of the `sh` child, which is also its process-group id.
    pub pid: u32,
    /// Child stdin, shared so concurrent `send`s queue instead of failing.
    pub stdin: Option<Arc<Mutex<ChildStdin>>>,
    /// Exit status, written only by the waiter thread.
    pub exit: ExitCell,
}

impl Session {
    /// Exit code once the waiter has reaped the child, else `None`.
    pub(crate) fn exit_code(&self) -> Option<i32> {
        *self.exit.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `true` once the child has been reaped.
    pub(crate) fn is_terminal(&self) -> bool {
        self.exit_code().is_some()
    }

    /// `"running"` or `"exited(<code>)"`, as the client parses it.
    pub(crate) fn status_str(&self) -> String {
        self.exit_code().map_or_else(|| "running".to_owned(), |code| format!("exited({code})"))
    }
}

/// Shell convention for an exit status: the exit code, or `128 + signal` when
/// the child was killed by a signal (so SIGKILL reads as 137, not -1).
pub(crate) fn exit_code_of(status: ExitStatus) -> i32 {
    status.code().or_else(|| status.signal().map(|sig| sig.saturating_add(128))).unwrap_or(-1)
}

/// Spawn `sh -c <command>` as the leader of a new process group, with stdout
/// and stderr appended to `log`, and start its waiter thread.
pub(crate) fn spawn(command: &str, cwd: Option<&str>, log: &std::fs::File) -> Result<Session, String> {
    let log_err = log.try_clone().map_err(|e| format!("Failed to clone log fd: {e}"))?;
    let log_out = log.try_clone().map_err(|e| format!("Failed to clone log fd: {e}"))?;

    let mut cmd = Command::new("sh");
    let _: &mut Command = cmd.args(["-c", command]).stdin(Stdio::piped()).stdout(log_out).stderr(log_err);
    // Own process group: signalling -pgid reaches pipelines and grandchildren.
    let _: &mut Command = cmd.process_group(0);
    if let Some(dir) = cwd {
        let _: &mut Command = cmd.current_dir(dir);
    }

    let mut child = cmd.spawn().map_err(|e| format!("Spawn failed: {e}"))?;
    let pid = child.id();
    let stdin = child.stdin.take().map(|s| Arc::new(Mutex::new(s)));
    let exit: ExitCell = Arc::new(Mutex::new(None));

    let cell = Arc::clone(&exit);
    drop(std::thread::spawn(move || {
        let code = child.wait().map_or(-1i32, exit_code_of);
        *cell.lock().unwrap_or_else(PoisonError::into_inner) = Some(code);
    }));

    Ok(Session { pid, stdin, exit })
}

/// `true` while at least one process of group `pgid` is alive.
fn group_alive(pgid: Pid) -> bool {
    killpg(pgid, None).is_ok()
}

/// SIGTERM the process group `pgid`, then SIGKILL it if anything in the group
/// is still alive after `grace_ms`.
///
/// Polls group liveness every 5 ms, so a group that exits promptly (the common
/// case) returns in a few ms. A missing group (`ESRCH`) returns at once.
pub(crate) fn terminate(pgid: u32, grace_ms: u64) {
    let Ok(raw) = i32::try_from(pgid) else { return };
    let group = Pid::from_raw(raw);
    if killpg(group, Signal::SIGTERM).is_err() {
        return;
    }
    let deadline = Instant::now().checked_add(Duration::from_millis(grace_ms));
    while group_alive(group) {
        if deadline.is_none_or(|d| Instant::now() >= d) {
            let _: Option<()> = killpg(group, Signal::SIGKILL).ok();
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Write `bytes` to a shared stdin, giving up after [`SEND_TIMEOUT`].
///
/// The write runs on a helper thread: a child that stops reading fills the
/// pipe and would block `write_all` forever. On timeout the helper stays
/// parked on the pipe (holding the stdin lock) until the child reads or dies.
pub(crate) fn write_with_timeout(stdin: Arc<Mutex<ChildStdin>>, bytes: Vec<u8>) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    drop(std::thread::spawn(move || {
        let mut pipe = stdin.lock().unwrap_or_else(PoisonError::into_inner);
        let result = pipe.write_all(&bytes).and_then(|()| pipe.flush());
        drop(pipe);
        drop(tx.send(result));
    }));
    match rx.recv_timeout(SEND_TIMEOUT) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(format!("Write failed: {e}")),
        Err(_) => Err("Write timed out: the process is not reading its stdin".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `sh -c script` to completion and map its status.
    fn code_of(script: &str) -> i32 {
        let status = Command::new("sh").args(["-c", script]).status().expect("sh runs");
        exit_code_of(status)
    }

    #[test]
    fn normal_exit_keeps_its_code() {
        assert_eq!(code_of("exit 3"), 3i32, "plain exit code passes through");
    }

    #[test]
    fn signal_death_maps_to_128_plus_signal() {
        assert_eq!(code_of("kill -9 $$"), 137i32, "SIGKILL must read 137, not -1");
    }
}
