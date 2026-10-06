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
    /// Whether the project root is inside a git repository.
    pub is_repo: bool,
    /// Current branch (`None` = detached HEAD or not a repo).
    pub branch: Option<String>,
    /// File-level diff stats against the requested base.
    pub file_changes: Vec<GitFileChange>,
}

/// Channel for handing `diff_base` requests to the worker thread.
type RequestTx = Sender<Option<String>>;

/// Shared worker handles: the request sender + the latest finished snapshot.
struct Worker {
    /// Drop a `diff_base` here to ask for a fresh computation.
    request_tx: RequestTx,
    /// Latest finished snapshot (`None` until the first run completes).
    latest: &'static Mutex<Option<GitSnapshot>>,
    /// `true` between sending a request and the worker storing its result.
    in_flight: &'static Mutex<bool>,
}

/// Lazily-spawned singleton worker.
static WORKER: OnceLock<Worker> = OnceLock::new();

/// Spawn the worker thread once and return the shared handles.
fn worker() -> &'static Worker {
    WORKER.get_or_init(|| {
        let (request_tx, request_rx): (RequestTx, Receiver<Option<String>>) = std::sync::mpsc::channel();
        let latest: &'static Mutex<Option<GitSnapshot>> = Box::leak(Box::new(Mutex::new(None)));
        let in_flight: &'static Mutex<bool> = Box::leak(Box::new(Mutex::new(false)));

        let _handle = thread::Builder::new().name("git-status".to_owned()).spawn(move || {
            // Coalesce bursts: if several requests queued while we computed,
            // keep only the most recent diff_base.
            while let Ok(mut diff_base) = request_rx.recv() {
                while let Ok(newer) = request_rx.try_recv() {
                    diff_base = newer;
                }
                let snapshot = compute_git_snapshot(diff_base.as_deref());
                if let Ok(mut slot) = latest.lock() {
                    *slot = Some(snapshot);
                }
                if let Ok(mut flag) = in_flight.lock() {
                    *flag = false;
                }
            }
        });

        Worker { request_tx, latest, in_flight }
    })
}

/// Take the latest finished snapshot, if a new one is ready.
///
/// Returns `None` when nothing has completed since the last call, so the
/// caller can keep its current state untouched.
#[must_use]
pub fn take_latest() -> Option<GitSnapshot> {
    let mut slot = worker().latest.lock().ok()?;
    slot.take()
}

/// Ask the worker for a fresh snapshot unless one is already being computed.
///
/// Non-blocking: dropping the request onto the channel is all it does.
pub fn request(diff_base: Option<String>) {
    let w = worker();
    let Ok(mut flag) = w.in_flight.lock() else {
        return;
    };
    if *flag {
        return;
    }
    *flag = true;
    drop(flag);
    if w.request_tx.send(diff_base).is_err() {
        // Worker gone (process shutting down) — clear the guard so a later
        // attempt is not wedged.
        if let Ok(mut reset) = w.in_flight.lock() {
            *reset = false;
        }
    }
}
