//! Meilisearch supervision: a single-flight spawn lock and a per-agent
//! watchdog thread that keeps the global server alive.
//!
//! ## Why this exists
//!
//! [`super::server::ensure_server_running`] only runs **once** per agent, at
//! module load (boot/reload). Nothing watched the server afterwards, so if the
//! global Meilisearch process died mid-session (OOM under an embedding burst, an
//! OS `kill`, a laptop sleep/resume severing it, a crash) it stayed dead until an
//! agent rebooted — and the cockpit vitals correctly, but uselessly, reported it
//! "down" with no path back up. In a deployment we can't hand-restart it.
//!
//! Two mechanisms close that gap:
//!
//! 1. [`SpawnLock`] — a machine-wide, single-flight guard around the spawn
//!    sequence. When the server dies, every agent's watchdog notices at roughly
//!    the same moment and races to respawn it; without coordination they'd spawn
//!    N processes that fight over the port and leave the pid/port files pointing
//!    at a loser that immediately exits. The lock elects exactly one spawner; the
//!    losers fall back to reconnecting to the winner. No external dependency — a
//!    POSIX-atomic `create_new` lockfile with a stale-steal escape hatch.
//!
//! 2. [`run`] — the watchdog loop. One per agent: every [`WATCHDOG_INTERVAL`] it
//!    health-checks the server and, on failure, drives a guarded respawn (which
//!    rebinds the **same** port — see [`super::server::ensure_server_running`] —
//!    so every agent's cached port stays valid and the blip is transparent).
//!    Consecutive failures back off so a permanently-broken server (missing
//!    binary, port stolen) doesn't hot-loop.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::server;

/// How often the watchdog probes server health.
const WATCHDOG_INTERVAL: Duration = Duration::from_secs(5);

/// Base back-off after a failed respawn; grows geometrically per consecutive
/// failure up to [`MAX_BACKOFF`] so a permanently-broken server can't hot-loop.
const BASE_BACKOFF: Duration = Duration::from_secs(2);

/// Cap on the watchdog respawn back-off.
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// A held spawn lock that is leaked (treated as stale) after this long, so a
/// crash mid-spawn never wedges every future respawn forever.
const LOCK_STALE: Duration = Duration::from_secs(30);

/// Max attempts to acquire-or-defer the spawn lock before giving up a respawn.
const LOCK_ATTEMPTS: u32 = 50;

/// Pause between spawn-lock acquisition attempts while another agent spawns.
const LOCK_POLL: Duration = Duration::from_millis(200);

// -- Single-flight spawn lock ------------------------------------------------

/// Path to the machine-wide spawn lock: `~/.context-pilot/meilisearch/spawn.lock`.
fn lock_path() -> Option<PathBuf> {
    server::global_meili_dir().ok().map(|d| d.join("spawn.lock"))
}

/// RAII guard over the machine-wide Meilisearch spawn lock.
///
/// Acquired via an atomic `create_new` open of `spawn.lock` (POSIX-exclusive, so
/// exactly one holder machine-wide). [`Drop`] removes the file. A lock older than
/// [`LOCK_STALE`] is considered abandoned (its holder crashed mid-spawn) and is
/// stolen, so the mechanism is self-healing rather than a permanent wedge.
pub(super) struct SpawnLock {
    /// The lock file path, removed on drop.
    path: PathBuf,
}

impl SpawnLock {
    /// Try to acquire the lock once (no waiting).
    ///
    /// Returns `Some(guard)` if we won it (freshly created, or stolen because the
    /// previous holder's file was older than [`LOCK_STALE`]), `None` if another
    /// live holder currently owns it.
    pub(super) fn try_acquire() -> Option<Self> {
        let path = lock_path()?;

        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                // Stamp our pid for diagnostics; failure is non-fatal (the file's
                // existence is the lock, not its contents).
                let _w = writeln!(f, "{}", std::process::id());
                Some(Self { path })
            }
            Err(_e) => {
                // Already held — steal it only if it looks abandoned (stale mtime).
                if Self::is_stale(&path) {
                    let _r = std::fs::remove_file(&path);
                    // One more attempt after stealing; if we still lose, defer.
                    OpenOptions::new().write(true).create_new(true).open(&path).ok().map(|mut f| {
                        let _w = writeln!(f, "{}", std::process::id());
                        Self { path }
                    })
                } else {
                    None
                }
            }
        }
    }

    /// Whether the lock file is older than [`LOCK_STALE`] (holder presumed dead).
    ///
    /// A file we can't stat is conservatively treated as **not** stale (don't
    /// steal what we can't reason about), so a transient stat error never causes
    /// a double-spawn.
    fn is_stale(path: &std::path::Path) -> bool {
        std::fs::metadata(path)
            .and_then(|m| m.modified())
            .is_ok_and(|mtime| mtime.elapsed().unwrap_or_default() > LOCK_STALE)
    }
}

impl Drop for SpawnLock {
    fn drop(&mut self) {
        let _r = std::fs::remove_file(&self.path);
    }
}

// -- Watchdog thread ---------------------------------------------------------

/// Handle to a running watchdog thread. Dropping it stops the thread (and joins
/// it), so a TUI reload that replaces `SearchState` tears the old watchdog down
/// cleanly instead of stacking a second one.
pub(crate) struct WatchdogHandle {
    /// Guarded inner so the handle is `Sync` (stored in `State`'s `TypeMap`).
    inner: std::sync::Mutex<WatchdogInner>,
}

/// Stop-signal + join handle, mutated only under the [`WatchdogHandle`] mutex.
struct WatchdogInner {
    /// Send `()` (or drop) to ask the loop to stop at the next tick.
    stop: mpsc::Sender<()>,
    /// The watchdog thread, joined on drop.
    join: Option<JoinHandle<()>>,
}

impl WatchdogHandle {
    /// Spawn a watchdog for the global server reachable at `port`/`master_key`.
    ///
    /// The cadence is [`WATCHDOG_INTERVAL`]; on a failed health probe it drives a
    /// single-flight respawn via [`respawn`]. The port/key are captured by value
    /// — they stay valid across respawns because the server rebinds the same
    /// port (stable-port policy in [`super::server`]).
    pub(crate) fn spawn(port: u16, master_key: String) -> Self {
        let (stop, rx) = mpsc::channel::<()>();
        let join = std::thread::Builder::new()
            .name("meili-watchdog".to_owned())
            .spawn(move || run(&rx, port, &master_key))
            .ok();
        Self { inner: std::sync::Mutex::new(WatchdogInner { stop, join }) }
    }
}

impl Drop for WatchdogHandle {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.inner.lock() {
            // Signal stop; ignore send error (loop may already be gone).
            let _s = inner.stop.send(());
            if let Some(handle) = inner.join.take() {
                let _j = handle.join();
            }
        }
    }
}

impl std::fmt::Debug for WatchdogHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WatchdogHandle(..)")
    }
}

/// The watchdog loop: probe health every [`WATCHDOG_INTERVAL`]; respawn on death.
///
/// Exits when the stop channel is signalled or its sender is dropped
/// (`recv_timeout` → `Disconnected`). Back-off grows with consecutive respawn
/// failures so a permanently-broken server is retried calmly, not hot-looped.
fn run(rx: &mpsc::Receiver<()>, port: u16, master_key: &str) {
    let mut consecutive_failures: u32 = 0;

    loop {
        // Sleep one interval, but wake immediately on a stop signal.
        match rx.recv_timeout(WATCHDOG_INTERVAL) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        if server::health_ok(port, master_key) {
            consecutive_failures = 0;
            continue;
        }

        log::warn!("Meilisearch health probe failed (port {port}) — attempting respawn");
        match respawn() {
            Ok(()) => {
                log::info!("Meilisearch watchdog recovered the server (port {port})");
                consecutive_failures = 0;
            }
            Err(e) => {
                consecutive_failures = consecutive_failures.saturating_add(1);
                let backoff = backoff_for(consecutive_failures);
                log::warn!(
                    "Meilisearch respawn failed ({e}); backing off {backoff:?} (failure #{consecutive_failures})"
                );
                // Honour the stop signal during the back-off too.
                match rx.recv_timeout(backoff) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        }
    }
}

/// Geometric back-off `BASE_BACKOFF * 2^(n-1)`, capped at [`MAX_BACKOFF`].
fn backoff_for(consecutive_failures: u32) -> Duration {
    let shift = consecutive_failures.saturating_sub(1).min(6);
    BASE_BACKOFF.checked_mul(1u32 << shift).unwrap_or(MAX_BACKOFF).min(MAX_BACKOFF)
}

/// Drive one single-flight respawn of the global server.
///
/// Holds the [`SpawnLock`] across the whole reconnect-or-spawn so that when N
/// agents detect the death together, exactly one spawns and the rest reconnect
/// to the winner. If another agent already holds the lock we briefly defer, then
/// re-check health — by then the winner has usually brought the server back, so
/// we return `Ok` without spawning anything ourselves.
fn respawn() -> Result<(), String> {
    let start = Instant::now();

    loop {
        if let Some(_guard) = SpawnLock::try_acquire() {
            // We're the elected spawner. Re-check first: the server may have come
            // back while we were contending, in which case reconnect is enough.
            return server::ensure_server_running().map(|_info| ());
        }

        // Someone else is spawning. Wait a beat, then see if the server is back.
        std::thread::sleep(LOCK_POLL);
        if server::reconnect_ok() {
            return Ok(());
        }

        if start.elapsed() > LOCK_POLL.saturating_mul(LOCK_ATTEMPTS) {
            return Err("timed out waiting for another agent to respawn Meilisearch".to_owned());
        }
    }
}
