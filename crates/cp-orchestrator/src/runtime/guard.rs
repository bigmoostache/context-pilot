//! Daemon resilience: one bad request or one panicking tick must never take
//! the whole backend down.
//!
//! Two failure modes this module closes:
//!
//! * **Poisoned backend lock.** Every handler shares one `Mutex<Backend>`. A
//!   panic while a thread holds it poisons it, and from then on every
//!   `state.lock()` returns `Err`. Handlers answer `500 backend lock poisoned`
//!   and the driver skips its work, so the daemon stays up but does nothing.
//!   [`run_guarded`] catches the panic at the thread boundary and clears the
//!   poison so the next caller proceeds.
//! * **Unbounded thread-per-request.** `tiny_http` hands each request to the
//!   caller, and the transport spawns one thread per request. Without a cap, a
//!   burst of requests (or many SSE tabs, each pinning a thread for its
//!   lifetime) grows threads without limit. [`Admission`] bounds the number
//!   in flight; over the cap the caller sheds load with `503`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Run `work`, and if it panics, log it and clear `state`'s poison flag.
///
/// Clearing the poison accepts that the panicking thread may have left the
/// backend half-updated. That is the lesser evil: the state is a projection
/// rebuilt from disk by the driver, whereas a permanently poisoned lock turns
/// every later request into a `500` until someone restarts the daemon.
pub(crate) fn run_guarded<T>(label: &str, state: &Mutex<T>, work: impl FnOnce()) {
    if catch_unwind(AssertUnwindSafe(work)).is_err() {
        crate::oerr!("{label}: thread panicked; backend lock recovered");
        state.clear_poison();
    }
}

/// Counter bounding how many request threads run at once.
#[derive(Debug)]
pub(crate) struct Admission {
    /// Requests currently holding a [`Permit`].
    in_flight: AtomicUsize,
    /// Most permits handed out at once.
    cap: usize,
}

/// Proof that a request was admitted; releases its slot when dropped, so a
/// panicking handler still gives the slot back during unwind.
#[derive(Debug)]
pub(crate) struct Permit(Arc<Admission>);

impl Admission {
    /// An admission gate allowing at most `cap` concurrent permits.
    pub(crate) const fn new(cap: usize) -> Self {
        Self { in_flight: AtomicUsize::new(0), cap }
    }

    /// Take a slot, or `None` when `cap` permits are already out.
    pub(crate) fn try_enter(self: &Arc<Self>) -> Option<Permit> {
        self.in_flight
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < self.cap).then_some(n.saturating_add(1)))
            .ok()
            .map(|_| Permit(Arc::clone(self)))
    }

    /// Permits currently held.
    #[cfg(test)]
    pub(crate) fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let _prev = self.0.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A panic while holding the lock poisons it; `run_guarded` must leave it
    /// usable for the next caller.
    #[test]
    fn panic_under_lock_is_recovered() {
        let state = Arc::new(Mutex::new(0u32));
        let shared = Arc::clone(&state);
        let joined = std::thread::spawn(move || {
            run_guarded("test", &shared, || {
                *shared.lock().expect("fresh lock") = 7;
                let _held = shared.lock().expect("fresh lock");
                panic!("boom while holding the lock");
            });
        })
        .join();
        assert!(joined.is_ok(), "run_guarded must swallow the panic");
        assert_eq!(*state.lock().expect("poison must be cleared"), 7);
    }

    /// Without the guard the same panic leaves the lock poisoned, which is the
    /// failure being fixed.
    #[test]
    fn unguarded_panic_poisons() {
        let state = Arc::new(Mutex::new(0u32));
        let shared = Arc::clone(&state);
        let _joined = std::thread::spawn(move || {
            let _guard = shared.lock().expect("fresh lock");
            panic!("boom");
        })
        .join();
        let _poisoned = state.lock().unwrap_err();
    }

    /// The gate admits up to `cap`, refuses beyond it, and frees a slot when a
    /// permit drops.
    #[test]
    fn admission_caps_and_releases() {
        let gate = Arc::new(Admission::new(2));
        let first = gate.try_enter().expect("slot 1");
        let _second = gate.try_enter().expect("slot 2");
        assert!(gate.try_enter().is_none(), "third must be refused");
        drop(first);
        assert_eq!(gate.in_flight(), 1);
        assert!(gate.try_enter().is_some(), "freed slot is reusable");
    }

    /// A panicking holder still releases its slot.
    #[test]
    fn permit_released_on_panic() {
        let gate = Arc::new(Admission::new(1));
        let permit = gate.try_enter().expect("slot");
        let _joined = std::thread::spawn(move || {
            let _held = permit;
            panic!("handler blew up");
        })
        .join();
        assert_eq!(gate.in_flight(), 0);
    }
}
