//! `bridge.lock` acquisition — the single-process folder gate for [`Boot`].
//!
//! Split from [`super`] (`boot/mod.rs`) to keep each file within the 500-line
//! budget. This module owns the `flock` on `<folder>/bridge.lock` plus the
//! bounded retry policy that lets a reload's replacement process win the lock
//! once its dying predecessor finally exits.
//!
//! [`Boot`]: super::Boot

use std::fs::{File, OpenOptions};
use std::path::Path;

use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};

use crate::error::{BootResult, Error};

/// Name of the lock file inside the agent folder whose `flock` gates
/// single-process ownership.
const LOCK_FILE: &str = "bridge.lock";

/// How many times [`acquire_lock`] re-attempts a contended `flock` before
/// giving up and declaring the folder already owned.
pub(super) const LOCK_RETRY_ATTEMPTS: u32 = 25;

/// Pause between contended `flock` attempts. `ATTEMPTS × BACKOFF` (~2s) is the
/// total grace window — comfortably longer than a clean process shutdown.
pub(super) const LOCK_RETRY_BACKOFF: std::time::Duration = std::time::Duration::from_millis(80);

/// Open `<folder>/bridge.lock` and take an exclusive `flock`, retrying briefly
/// on contention.
///
/// The OS releases an `flock` the instant its holder dies (even on `SIGKILL`),
/// so contention means a process is *still alive* holding it. During a reload
/// the supervisor can spawn the replacement before the outgoing process has
/// finished exiting — a genuine instance, but a transient one. A plain
/// non-blocking lock would lose that race and boot the bridge OFF, leaving the
/// agent unreachable until the next manual restart.
///
/// So we retry the non-blocking lock up to `max_retries` times with a
/// [`LOCK_RETRY_BACKOFF`] pause between attempts. The patient startup path
/// passes [`LOCK_RETRY_ATTEMPTS`] (~2s total); the fail-fast background
/// recovery path passes `0` (a single attempt, no sleep) so it never stalls the
/// main loop. A *blocking* lock is deliberately avoided: were the previous
/// process to hang forever, it would wedge boot indefinitely — the bounded
/// retry waits out the common case, then refuses cleanly with
/// [`Error::AlreadyRunning`] for a truly persistent owner.
///
/// Any non-contention `flock` error is a genuine I/O fault and is returned
/// immediately without retry.
///
/// # Errors
///
/// [`Error::AlreadyRunning`] if the lock stays contended past `max_retries`,
/// or [`Error::Io`] for an `open`/`flock` I/O fault.
pub(super) fn acquire_lock(folder: &Path, max_retries: u32) -> BootResult<Flock<File>> {
    let lock_path = folder.join(LOCK_FILE);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| Error::io(format!("open lock {}", lock_path.display()), e))?;

    let mut attempt: u32 = 0;
    loop {
        match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
            Ok(lock) => return Ok(lock),
            // `Flock::lock` hands the `File` back on failure so we can re-try.
            Err((returned, errno)) => {
                let contended = errno == Errno::EAGAIN || errno == Errno::EWOULDBLOCK;
                if contended && attempt < max_retries {
                    attempt = attempt.saturating_add(1);
                    std::thread::sleep(LOCK_RETRY_BACKOFF);
                    file = returned;
                    continue;
                }
                return Err(if contended {
                    Error::AlreadyRunning { folder: folder.to_string_lossy().into_owned() }
                } else {
                    Error::io(format!("flock {}", lock_path.display()), errno.into())
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn acquire_lock_retries_until_holder_releases() {
        use std::sync::mpsc;
        use std::time::Duration;
        use tempfile::tempdir;

        let folder = tempdir().expect("folder");
        let canonical = fs::canonicalize(folder.path()).expect("canonicalise");

        // A holder thread grabs the lock, signals that it holds it, then waits
        // a beat (shorter than the retry budget) and releases — modelling the
        // outgoing process of a reload finishing its shutdown.
        let (held_tx, held_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let holder_path = canonical.clone();
        let holder = std::thread::spawn(move || {
            let lock = acquire_lock(&holder_path, LOCK_RETRY_ATTEMPTS).expect("holder acquires");
            held_tx.send(()).expect("signal held");
            // Hold until told to release, then drop the lock.
            let _ = release_rx.recv();
            drop(lock);
        });

        held_rx.recv().expect("holder signalled");

        // While the holder still owns the lock, a contender starts retrying.
        let contender_path = canonical.clone();
        let contender = std::thread::spawn(move || acquire_lock(&contender_path, LOCK_RETRY_ATTEMPTS));

        // Let the contender spin on contention for a couple of cycles, then
        // release the holder — well within the ~2s retry budget.
        std::thread::sleep(Duration::from_millis(200));
        release_tx.send(()).expect("trigger release");

        let acquired = contender.join().expect("contender thread");
        holder.join().expect("holder thread");
        assert!(acquired.is_ok(), "the contender must win the lock once the holder releases, got {acquired:?}",);
    }
}
