//! [`Boot`] — the agent-side boot sequence and the resources it holds.
//!
//! When the bridge is switched ON, exactly one process per agent folder must
//! own the agent's runtime resources. [`Boot::start`] acquires them in a
//! deliberate order so that the **single-process gate is the very first step**
//! and nothing else races ahead of it:
//!
//! 1. **Folder lock** — an exclusive, non-blocking `flock` on `<folder>/bridge.lock`.
//!    A second instance in the same folder fails here with
//!    [`Error::AlreadyRunning`] (single-process exclusion, design doc I1/D2).
//!    Contention is retried briefly (~2s) so a reload's replacement process can
//!    win the lock once the outgoing process finishes exiting, rather than
//!    booting bridge-OFF and leaving the agent unreachable. The lock is held
//!    for the lifetime of the [`Boot`] and released on drop.
//! 2. **Oplog** — open (creating if absent) the agent's durable log via
//!    [`OplogService`], which also spawns the off-loop group-commit thread.
//! 3. **Stream socket** — bind `<folder>/stream.sock` (unlinking a stale socket
//!    a previous crash left behind). The stream tee (Phase 12) and command
//!    intake (Phase 13) will accept on it; boot only binds and holds it.
//! 4. **Identity** — mint a fresh 256-bit `cap_token` and 128-bit `boot_id`.
//! 5. **Registry** — write `~/.context-pilot/agents/<id>.json` **last**, atomically
//!    and `0600` (design doc §10). Writing it last means any earlier failure
//!    leaves no discovery record pointing at half-acquired resources.
//!
//! On drop, the registry record and the socket file are removed (best-effort)
//! so the backend observes a clean disappearance; the lock and the oplog thread
//! are released by their own `Drop`.
//!
//! # H5 — FD inheritance across a deadman re-exec (deferred)
//!
//! The lock fd is **not** made inheritable here: the std file carries
//! `FD_CLOEXEC`, so a child process the agent spawns cannot accidentally keep
//! the lock alive after the agent dies. The deadman re-exec (a later phase)
//! that must inherit the held lock without a gap will explicitly clear
//! `FD_CLOEXEC` on the lock fd at the point of re-exec; doing it here would risk
//! leaking the lock into unrelated children.

use std::fs::{self, File, OpenOptions};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};

use cp_oplog::service::Service as OplogService;
use cp_wire::heartbeat::DEFAULT_CADENCE;
use cp_wire::types::oplog::OpEntryKind;
use cp_wire::types::registry::{AgentStatus, Entry};
use cp_wire::types::LifecycleState;
use cp_wire::PROTOCOL_VERSION;

use crate::error::{Error, BootResult};
use crate::heartbeat::Beacon;
use crate::register::identity::{folder_id, mint_boot_id, mint_cap_token};
use crate::register::registry;

/// Name of the lock file inside the agent folder whose `flock` gates
/// single-process ownership.
const LOCK_FILE: &str = "bridge.lock";

/// Name of the agent's oplog directory inside the agent folder.
const OPLOG_DIR: &str = "oplog";

/// Name of the agent's stream socket inside the agent folder.
const SOCKET_FILE: &str = "stream.sock";

/// Name of the agent's heartbeat file inside the agent folder (written by the
/// Phase 11 heartbeat thread; boot only records its path).
const HEARTBEAT_FILE: &str = "heartbeat";

/// A booted agent bridge: the held resources plus the registry record that
/// advertises them.
///
/// Dropping it releases the folder lock, stops the oplog commit thread, and
/// removes the registry record and socket file.
#[derive(Debug)]
pub struct Boot {
    /// The held exclusive folder lock — released on drop. The leading
    /// underscore documents that it is owned for its `Drop`, not read.
    _lock: Flock<File>,

    /// The agent's durable oplog plus its group-commit thread.
    oplog: OplogService,

    /// The bound stream socket the publisher/intake will accept on.
    listener: UnixListener,

    /// The discovery record written to the agents directory.
    entry: Entry,

    /// The agents directory the registry record lives in (for drop cleanup).
    agents_dir: PathBuf,

    /// The bound socket's path (for drop cleanup).
    socket_path: PathBuf,

    /// The liveness beacon thread — stopped and joined on drop. The leading
    /// underscore documents that it is owned for its `Drop`, not read.
    _heartbeat: Beacon,
}

impl Boot {
    /// Boot the bridge for `folder`, advertising `model`, writing the registry
    /// into the default `~/.context-pilot/agents` directory.
    ///
    /// # Errors
    ///
    /// Returns [`Error::AlreadyRunning`] if another live agent already owns
    /// `folder`, or [`Error::Io`] for any filesystem failure (lock, oplog,
    /// socket, registry) — or if `$HOME` is unset.
    pub fn start(folder: &Path, model: &str) -> BootResult<Self> {
        let agents_dir = registry::default_agents_dir()?;
        Self::start_in(folder, &agents_dir, model)
    }

    /// Attempt a boot with a **single, non-blocking** lock acquisition — no
    /// retry-on-contention wait.
    ///
    /// Used by the main-loop background *recovery* path: if the bridge boot
    /// failed at startup (e.g. a relaunch lost the `flock` race to a still-dying
    /// predecessor), the loop re-attempts boot periodically. Each attempt must
    /// return *immediately* so it never stalls the loop — so on contention this
    /// fails fast with [`Error::AlreadyRunning`] rather than sleeping out the
    /// ~2s retry window. The next retry tick tries again; once the predecessor
    /// finally dies and frees the lock, an attempt wins and the bridge comes up
    /// live mid-session.
    ///
    /// # Errors
    ///
    /// [`Error::AlreadyRunning`] immediately if the folder lock is contended,
    /// or [`Error::Io`] for any filesystem failure.
    pub fn try_start(folder: &Path, model: &str) -> BootResult<Self> {
        let agents_dir = registry::default_agents_dir()?;
        Self::start_inner(folder, &agents_dir, model, 0)
    }

    /// Boot the bridge writing the registry into an explicit `agents_dir`
    /// (tests point this at a tempdir so they never touch the real home).
    ///
    /// # Errors
    ///
    /// As [`start`](Self::start).
    pub fn start_in(folder: &Path, agents_dir: &Path, model: &str) -> BootResult<Self> {
        Self::start_inner(folder, agents_dir, model, LOCK_RETRY_ATTEMPTS)
    }

    /// Shared boot body. `lock_attempts` is the number of *additional* contended
    /// `flock` retries: [`LOCK_RETRY_ATTEMPTS`] for the patient startup path, `0`
    /// for the fail-fast [`try_start`](Self::try_start) recovery path.
    ///
    /// # Errors
    ///
    /// As [`start`](Self::start).
    fn start_inner(
        folder: &Path,
        agents_dir: &Path,
        model: &str,
        lock_attempts: u32,
    ) -> BootResult<Self> {
        // The folder must exist before we can canonicalise + lock it.
        fs::create_dir_all(folder)
            .map_err(|e| Error::io(format!("create agent folder {}", folder.display()), e))?;
        let canonical = fs::canonicalize(folder)
            .map_err(|e| Error::io(format!("canonicalise {}", folder.display()), e))?;
        let id = folder_id(&canonical.to_string_lossy());

        // 1. The single-process gate — must come first.
        let lock = acquire_lock(&canonical, lock_attempts)?;

        // 2. Oplog (opens/creates the dir, spawns the commit thread).
        let oplog_path = canonical.join(OPLOG_DIR);
        let oplog = OplogService::spawn(&oplog_path)
            .map_err(|e| Error::io(format!("open oplog {}", oplog_path.display()), into_io(&e)))?;

        // 3. Stream socket — unlink any stale socket a crash left behind.
        let socket_path = canonical.join(SOCKET_FILE);
        let _ignored = fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path)
            .map_err(|e| Error::io(format!("bind socket {}", socket_path.display()), e))?;

        // 4. Identity secrets.
        let cap_token = mint_cap_token()?;
        let boot_id = mint_boot_id()?;

        // 5. Registry record, written last and atomically.
        let entry = Entry {
            schema_version: 1,
            id,
            folder: canonical.to_string_lossy().into_owned(),
            pid: std::process::id(),
            boot_id,
            model: model.to_owned(),
            protocol_version: PROTOCOL_VERSION,
            binary_version: env!("CARGO_PKG_VERSION").to_owned(),
            socket_path: socket_path.to_string_lossy().into_owned(),
            oplog_path: oplog_path.to_string_lossy().into_owned(),
            heartbeat_path: canonical.join(HEARTBEAT_FILE).to_string_lossy().into_owned(),
            cap_token,
            started_at_ms: now_ms(),
            status: AgentStatus::Starting,
        };
        let _written = registry::write_entry(agents_dir, &entry)?;

        // The liveness beacon starts last, once every advertised resource
        // exists: it writes the first beat synchronously, so the moment this
        // returns the backend can both discover (registry) and verify (beat).
        let heartbeat = Beacon::start(
            Path::new(&entry.heartbeat_path),
            entry.pid,
            entry.boot_id.clone(),
            DEFAULT_CADENCE,
        )
        .map_err(|e| Error::io(format!("start heartbeat {}", entry.heartbeat_path), e))?;

        // Authoritative "operational" signal on the oplog (I8): now that every
        // advertised resource exists, journal a durable `Lifecycle::Running` so
        // the backend's view folds an authoritative liveness fact rather than
        // inferring it from the heartbeat. Non-blocking durable (group-committed
        // off-loop, never fsyncs an interactive path). Symmetric with the
        // `Lifecycle::Stopping` emitted on `Drop`.
        oplog.submit_durable(OpEntryKind::Lifecycle { state: LifecycleState::Running });

        Ok(Self {
            _lock: lock,
            oplog,
            listener,
            entry,
            agents_dir: agents_dir.to_path_buf(),
            socket_path,
            _heartbeat: heartbeat,
        })
    }

    /// The agent's stable registry id (FNV-1a of its canonical folder path).
    #[must_use]
    pub fn id(&self) -> &str {
        &self.entry.id
    }

    /// The bearer capability token a commander must present (design doc I9).
    #[must_use]
    pub fn cap_token(&self) -> &str {
        &self.entry.cap_token
    }

    /// The discovery record this boot advertised.
    #[must_use]
    pub const fn entry(&self) -> &Entry {
        &self.entry
    }

    /// The agent's durable oplog service.
    #[must_use]
    pub const fn oplog(&self) -> &OplogService {
        &self.oplog
    }

    /// The bound stream socket listener.
    #[must_use]
    pub const fn listener(&self) -> &UnixListener {
        &self.listener
    }
}

impl Drop for Boot {
    /// Remove the discovery record and socket file so the backend sees a clean
    /// disappearance. The lock and oplog thread release via their own `Drop`.
    ///
    /// Before tearing down, append a durable [`Lifecycle::Stopping`] delta so the
    /// backend's view records an *authoritative* graceful shutdown (I8) rather
    /// than inferring it from the heartbeat going stale. The append is the
    /// non-blocking durable path; the `oplog` field drops *after* this body, and
    /// its commit thread drains every queued job before joining
    /// (`Service::drop`), so the Stopping record is `fdatasync`'d before exit.
    /// On a hard kill (`SIGKILL`) this body never runs — the backend then falls
    /// back to liveness (flock release + stale heartbeat), which is the
    /// intended best-effort-graceful contract.
    ///
    /// [`Lifecycle::Stopping`]: cp_wire::types::LifecycleState::Stopping
    fn drop(&mut self) {
        self.oplog
            .submit_durable(OpEntryKind::Lifecycle { state: LifecycleState::Stopping });

        let registry = registry::path(&self.agents_dir, &self.entry.id);
        let _registry_removed = fs::remove_file(&registry);
        let _socket_removed = fs::remove_file(&self.socket_path);
    }
}

/// How many times [`acquire_lock`] re-attempts a contended `flock` before
/// giving up and declaring the folder already owned.
const LOCK_RETRY_ATTEMPTS: u32 = 25;

/// Pause between contended `flock` attempts. `ATTEMPTS × BACKOFF` (~2s) is the
/// total grace window — comfortably longer than a clean process shutdown.
const LOCK_RETRY_BACKOFF: std::time::Duration = std::time::Duration::from_millis(80);

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
fn acquire_lock(folder: &Path, max_retries: u32) -> BootResult<Flock<File>> {
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

/// Flatten an [`Error`](cp_oplog::error::Error) into an [`io::Error`]
/// so it can ride a [`Error::Io`] (the bridge treats an oplog open failure
/// as a filesystem fault for boot purposes).
fn into_io(e: &cp_oplog::error::Error) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

/// Wall-clock milliseconds since the Unix epoch, or `0` if the clock predates
/// it (the value is informational; liveness uses `boot_id`/heartbeat, not this).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Boot into temp folders so tests never touch the real home or cwd.
    fn boot(folder: &Path, agents: &Path) -> BootResult<Boot> {
        Boot::start_in(folder, agents, "test-model")
    }

    #[test]
    fn boot_acquires_all_resources() {
        let folder = tempdir().expect("folder");
        let agents = tempdir().expect("agents");
        let booted = boot(folder.path(), agents.path()).expect("boot");

        // Registry record exists, 0600, and round-trips.
        let registry = registry::path(agents.path(), booted.id());
        assert!(registry.exists(), "registry record written");

        // Socket bound, oplog dir created.
        assert!(folder.path().join(SOCKET_FILE).exists(), "socket bound");
        assert!(folder.path().join(OPLOG_DIR).exists(), "oplog dir created");

        // The advertised paths are inside the canonical folder.
        assert!(booted.entry().oplog_path.ends_with(OPLOG_DIR));
        assert_eq!(booted.entry().protocol_version, PROTOCOL_VERSION);
        assert_eq!(booted.cap_token().len(), 64, "256-bit token");
    }

    #[test]
    fn second_boot_same_folder_refuses() {
        let folder = tempdir().expect("folder");
        let agents = tempdir().expect("agents");
        let _first = boot(folder.path(), agents.path()).expect("first boot");

        let second = boot(folder.path(), agents.path());
        assert!(
            matches!(second, Err(Error::AlreadyRunning { .. })),
            "a second instance in the same folder must be refused, got {second:?}",
        );
    }

    #[test]
    fn boot_releases_lock_on_drop() {
        let folder = tempdir().expect("folder");
        let agents = tempdir().expect("agents");
        {
            let _first = boot(folder.path(), agents.path()).expect("first boot");
        } // dropped here → lock released, registry + socket removed.

        // A fresh boot in the same folder now succeeds.
        let again = boot(folder.path(), agents.path());
        assert!(again.is_ok(), "lock must be released on drop, got {again:?}");
    }

    #[test]
    fn drop_removes_registry_record() {
        let folder = tempdir().expect("folder");
        let agents = tempdir().expect("agents");
        let registry;
        {
            let booted = boot(folder.path(), agents.path()).expect("boot");
            registry = registry::path(agents.path(), booted.id());
            assert!(registry.exists());
        }
        assert!(!registry.exists(), "registry record removed on graceful drop");
    }

    #[test]
    fn stale_socket_is_replaced() {
        let folder = tempdir().expect("folder");
        let agents = tempdir().expect("agents");
        // Simulate a crash leaving a stale socket file.
        fs::create_dir_all(folder.path()).expect("mkdir");
        fs::write(folder.path().join(SOCKET_FILE), b"stale").expect("stale socket");

        let booted = boot(folder.path(), agents.path());
        assert!(booted.is_ok(), "a stale socket must be unlinked and rebound, got {booted:?}");
    }

    #[test]
    fn try_start_fails_fast_when_locked_then_recovers_when_freed() {
        use std::time::Instant;

        let folder = tempdir().expect("folder");
        let agents = tempdir().expect("agents");

        // A patient boot holds the lock.
        let first = boot(folder.path(), agents.path()).expect("first boot");

        // A fail-fast `try_start` against the same folder must refuse
        // *immediately* (no ~2s retry wait) with `AlreadyRunning` — this is the
        // background recovery path that must never stall the main loop.
        let started = Instant::now();
        let contended = Boot::start_inner(folder.path(), agents.path(), "test-model", 0);
        let elapsed = started.elapsed();
        assert!(
            matches!(contended, Err(Error::AlreadyRunning { .. })),
            "a contended fail-fast attempt must refuse, got {contended:?}",
        );
        assert!(
            elapsed < LOCK_RETRY_BACKOFF,
            "fail-fast must return well under one backoff ({elapsed:?}), not sleep out the retry window",
        );

        // Once the holder releases the lock, the next fail-fast attempt wins —
        // modelling the bridge recovering mid-session after a dying predecessor
        // finally frees the lock.
        drop(first);
        let recovered = Boot::start_inner(folder.path(), agents.path(), "test-model", 0);
        assert!(recovered.is_ok(), "fail-fast must succeed once the lock is free, got {recovered:?}");
    }

    #[test]
    fn acquire_lock_retries_until_holder_releases() {
        use std::sync::mpsc;
        use std::time::Duration;

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
        assert!(
            acquired.is_ok(),
            "the contender must win the lock once the holder releases, got {acquired:?}",
        );
    }

    /// Read every cleanly-decoded oplog entry across all segments in `dir`.
    fn read_all_entries(dir: &Path) -> Vec<cp_wire::types::oplog::OpEntry> {
        let mut out = Vec::new();
        for idx in cp_oplog::segment::indices(dir).unwrap_or_default() {
            if let Ok(scan) = cp_oplog::segment::read(&cp_oplog::segment::path(dir, idx)) {
                out.extend(scan.entries);
            }
        }
        out
    }

    #[test]
    fn lifecycle_running_on_boot_and_stopping_on_drop() {
        let folder = tempdir().expect("folder");
        let agents = tempdir().expect("agents");
        let oplog_dir = fs::canonicalize(folder.path()).expect("canon").join(OPLOG_DIR);

        // Boot emits Lifecycle::Running; dropping it emits Lifecycle::Stopping.
        // The drop joins the oplog commit thread, draining + fsyncing both
        // records before it returns, so reading after the drop is race-free.
        let booted = boot(folder.path(), agents.path()).expect("boot");
        drop(booted);

        let lifecycles: Vec<LifecycleState> = read_all_entries(&oplog_dir)
            .iter()
            .filter_map(|e| match &e.kind {
                OpEntryKind::Lifecycle { state } => Some(*state),
                _ => None,
            })
            .collect();

        assert!(
            lifecycles.contains(&LifecycleState::Running),
            "Lifecycle::Running must be journaled at boot, got {lifecycles:?}",
        );
        assert!(
            lifecycles.contains(&LifecycleState::Stopping),
            "Lifecycle::Stopping must be journaled on graceful drop, got {lifecycles:?}",
        );
    }
}
