//! Ordering between a mutating `gh` call and panel refreshes.
//!
//! A mutation marks its panels stale *before* it runs (it runs on a worker
//! thread). Without coordination a refresh could fetch the pre-mutation state
//! and then clear the stale flag. Two rules close that race:
//! - no `GithubResult` refresh starts while a mutation is in flight; the stale
//!   flag survives and the panel is retried once the mutation has landed;
//! - a refresh that was already running when a mutation started is reported as
//!   stale on arrival, so it is fetched again.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex, PoisonError};

/// Mutations currently running.
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// Mutations ever started; a refresh compares it before and after.
static STARTED: AtomicU64 = AtomicU64::new(0);

/// Panels whose last refresh overlapped a mutation start.
static OVERLAPPED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Held for the duration of one mutating `gh` call (released on drop, panics included).
#[derive(Debug)]
pub(crate) struct Guard(());

impl Drop for Guard {
    fn drop(&mut self) {
        let _prev = IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Register a mutation; keep the guard until the `gh` process has exited.
pub(crate) fn begin() -> Guard {
    let _started = STARTED.fetch_add(1, Ordering::AcqRel);
    let _running = IN_FLIGHT.fetch_add(1, Ordering::AcqRel);
    Guard(())
}

/// Whether a mutation is running (refreshes must wait).
pub(crate) fn in_flight() -> bool {
    IN_FLIGHT.load(Ordering::Acquire) > 0
}

/// Ticket taken when a refresh is built; compare with [`finish_refresh`].
pub(crate) fn ticket() -> u64 {
    STARTED.load(Ordering::Acquire)
}

/// Called when a refresh's `gh` call returns: flags the panel if a mutation
/// started meanwhile.
pub(crate) fn finish_refresh(context_id: &str, ticket: u64) {
    if STARTED.load(Ordering::Acquire) != ticket {
        let _new = OVERLAPPED.lock().unwrap_or_else(PoisonError::into_inner).insert(context_id.to_owned());
    }
}

/// Whether the refresh just applied to `context_id` may predate a mutation
/// (consumes the flag).
pub(crate) fn take_overlapped(context_id: &str) -> bool {
    OVERLAPPED.lock().unwrap_or_else(PoisonError::into_inner).remove(context_id)
}
