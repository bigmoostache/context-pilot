//! Auto-update scheduler glue (O4.2) — the background thread that polls the
//! channel on boot and every `poll_interval_hours`, and drives the M3 apply
//! pipeline when the tick decision (see
//! [`scheduler::run_tick`](crate::services::releases::updater)) says so.
//!
//! Lock discipline: the backend lock is held only to snapshot config and to
//! stage the (fast, local) apply — never across the network download. A
//! successful apply ends in [`restart_self`], so the loop's last act is
//! logging the decision; the health-gated committer of the next boot finishes
//! the job.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::services::ReleaseStore;
use crate::services::releases::updater::{
    UpdateEvaluation, check_stable, download_artifact, restart_self, scheduler, stage_apply,
};
use crate::transport::Backend;
// The process-wide apply gate is shared with `POST /api/update/apply` so the
// scheduler and an admin click can never race two applies.
use crate::transport::rest::APPLY_IN_FLIGHT;

/// Settle delay before the boot poll — lets the transport bind and the first
/// registry scan land so a check never races the boot sequence.
const BOOT_POLL_DELAY: Duration = Duration::from_secs(30);

/// Spawn the scheduler loop. One thread for the process lifetime.
pub(crate) fn spawn(backend: Arc<Mutex<Backend>>, auth_db: PathBuf, install: PathBuf) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        thread::sleep(BOOT_POLL_DELAY);
        loop {
            let sleep = tick(&backend, &auth_db, &install);
            thread::sleep(sleep);
        }
    })
}

/// One poll tick: snapshot config, run the decision, log it, and say how long
/// to sleep before the next tick (until the window opens when an update is
/// waiting on it, else the configured poll interval).
fn tick(backend: &Arc<Mutex<Backend>>, auth_db: &PathBuf, install: &PathBuf) -> Duration {
    // Snapshot everything the tick needs under one short lock.
    let Ok(b) = backend.lock() else {
        return Duration::from_secs(60);
    };
    let mode = b.releases.update_mode();
    let window = b.releases.window().clone();
    let interval_hours = b.releases.poll_interval_hours().max(1);
    let arch = b.releases.arch().to_owned();
    let releases_dir = b.releases.dir().to_path_buf();
    let current = scheduler::current_version(&b.releases);
    drop(b);

    let now_minutes = scheduler::local_now_minutes();
    let outcome = scheduler::run_tick(
        mode,
        &window,
        now_minutes,
        &APPLY_IN_FLIGHT,
        || {
            check_stable(&releases_dir, &current).map(|eval| match eval {
                UpdateEvaluation::Available(manifest) => Some(manifest),
                UpdateEvaluation::UpToDate => None,
            })
        },
        |manifest| {
            // Download outside the backend lock — long network I/O.
            let snapshot = ReleaseStore::load(releases_dir.clone());
            download_artifact(&snapshot, manifest, &arch)?;
            // Stage (DB backup + binary swap) under the lock — local + fast.
            let Ok(b) = backend.lock() else {
                return Err("backend lock poisoned".to_owned());
            };
            stage_apply(&b.releases, b.auth.as_ref(), auth_db, install, &manifest.version)?;
            drop(b);
            restart_self();
            Ok(current.clone())
        },
    );
    eprintln!("updater: {}", outcome.describe());

    match outcome {
        scheduler::TickOutcome::SkipWindow { .. } => {
            // Wake exactly when the window opens instead of gambling that a
            // fixed-interval tick lands inside it.
            let minutes = window.minutes_until_open(scheduler::local_now_minutes()).max(1);
            Duration::from_secs(u64::from(minutes) * 60)
        }
        _ => Duration::from_secs(u64::from(interval_hours) * 3600),
    }
}
