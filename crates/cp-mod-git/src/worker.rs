//! Background git-status worker.
//!
//! Running `git` touches the whole working tree and spawns several
//! subprocesses — on a large repo that is tens of milliseconds, far too much
//! for the single-threaded main loop. This module moves the work onto a
//! dedicated background thread: the loop only ever drops off a request and
//! picks up the latest finished [`GitSnapshot`], both O(µs) non-blocking
//! operations.
//!
//! The snapshot lags at most one refresh interval behind reality, which is
//! invisible for an overview panel that already refreshes on a timer.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread;

use crate::{GitFileChange, compute_git_snapshot};

/// Immutable result of one git-status computation.
#[derive(Debug, Clone, Default)]
pub struct GitSnapshot {
    /// Directory the snapshot was computed in (a thread's workdir).
    pub dir: std::path::PathBuf,
    /// Whether the project root is inside a git repository.
    pub is_repo: bool,
    /// Current branch (`None` = detached HEAD or not a repo).
    pub branch: Option<String>,
    /// File-level diff stats against the requested base.
    pub file_changes: Vec<GitFileChange>,
}

/// One request: directory to inspect + optional diff base.
type Request = (std::path::PathBuf, Option<String>);

/// Channel for handing requests to the worker thread.
type RequestTx = Sender<Request>;

/// Shared worker handles: the request sender + the latest finished snapshot.
struct Worker {
    /// Drop a request here to ask for a fresh computation.
    request_tx: RequestTx,
    /// Latest finished snapshot (`None` until the first run completes).
    latest: &'static Mutex<Option<GitSnapshot>>,
}

/// Lazily-spawned singleton worker.
static WORKER: OnceLock<Worker> = OnceLock::new();

/// Spawn the worker thread once and return the shared handles.
fn worker() -> &'static Worker {
    WORKER.get_or_init(|| {
        let (request_tx, request_rx): (RequestTx, Receiver<Request>) = std::sync::mpsc::channel();
        let latest: &'static Mutex<Option<GitSnapshot>> = Box::leak(Box::new(Mutex::new(None)));

        let _handle = thread::Builder::new().name("git-status".to_owned()).spawn(move || {
            // Coalesce bursts: if several requests queued while we computed,
            // keep only the most recent one, so a slow git never builds a backlog.
            while let Ok(mut req) = request_rx.recv() {
                while let Ok(newer) = request_rx.try_recv() {
                    req = newer;
                }
                // A panic must not kill the thread: status would freeze for the
                // rest of the process.
                let computed = std::panic::catch_unwind(|| compute_git_snapshot(&req.0, req.1.as_deref()));
                if let (Ok(snapshot), Ok(mut slot)) = (computed, latest.lock()) {
                    *slot = Some(snapshot);
                }
            }
        });

        Worker { request_tx, latest }
    })
}

/// Take the latest finished snapshot if it was computed for `dir`.
///
/// Returns `None` when nothing new is ready or the snapshot belongs to another
/// thread's workdir; that one stays in the slot for its owner to apply.
#[must_use]
pub fn take_latest_for(dir: &std::path::Path) -> Option<GitSnapshot> {
    let mut slot = worker().latest.lock().ok()?;
    slot.take_if(|s| s.dir == dir)
}

/// Ask the worker for a fresh snapshot.
///
/// Non-blocking: dropping the request onto the channel is all it does. Every
/// request is sent (no in-flight gate), so a request for another workdir is
/// never lost; the worker collapses a backlog to its newest entry.
pub fn request(dir: std::path::PathBuf, diff_base: Option<String>) {
    // Send fails only when the worker is gone (process shutting down).
    drop(worker().request_tx.send((dir, diff_base)));
}

/// Untracked files larger than this are not read to count lines (a stray
/// multi-GB dump would otherwise be read in full on every refresh).
const LINE_COUNT_MAX_BYTES: u64 = 1024 * 1024;

/// Line count of `path`, or 0 when it is unreadable, binary, or over
/// [`LINE_COUNT_MAX_BYTES`].
pub(crate) fn count_lines_capped(path: &std::path::Path) -> usize {
    let small = std::fs::metadata(path).is_ok_and(|m| m.len() <= LINE_COUNT_MAX_BYTES);
    if !small {
        return 0;
    }
    std::fs::read_to_string(path).map_or(0, |c| c.lines().count())
}

/// Per-call ceiling for status git commands: a hung git (stale lock, prompt)
/// must not stall every later refresh.
const STATUS_GIT_TIMEOUT_SECS: u64 = 10;

/// Run one status `git` subcommand in `dir`; `None` on spawn error, timeout or
/// non-zero exit. Inherited `GIT_DIR`/`GIT_WORK_TREE` are dropped so `dir`
/// decides the repo, prompts are disabled, and optional locks are skipped so
/// status never contends with the agent's own `git` calls on `index.lock`.
pub(crate) fn git_stdout(dir: &std::path::Path, args: &[&str]) -> Option<String> {
    let mut cmd = std::process::Command::new("git");
    let _c = cmd
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0");
    let out = cp_base::modules::run_with_timeout(cmd, STATUS_GIT_TIMEOUT_SECS).ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}
