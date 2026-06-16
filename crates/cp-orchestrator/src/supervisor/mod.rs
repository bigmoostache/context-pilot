//! Agent lifecycle **supervisor** — spawn, stop, restart, adopt.
//!
//! The [`AgentSupervisor`] is the backend's process manager.  It owns the
//! mapping from `agent_id` → running process and enforces the **allow-list
//! gate** (R2-15): only binaries whose `realpath` appears on the list may be
//! spawned.  Path canonicalisation (`std::fs::canonicalize`) defeats symlink
//! traversal and `..` escape.
//!
//! Spawned agents run **detached** (`process_group(0)` — new process group,
//! survives backend crash) with stdin/stdout/stderr closed.  The supervisor
//! never touches the agent's oplog or registry; it only manages the OS
//! process.
//!
//! # Adopt
//!
//! On backend restart, the registry may contain agents that are still live
//! (valid pid, fresh heartbeat).  [`adopt`](AgentSupervisor::adopt) records
//! them in the supervisor without re-spawning — the backend re-acquires
//! control of their lifecycle (D7).

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::io;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;

use cp_wire::types::registry::Entry;

// ── Constants ───────────────────────────────────────────────────────────

/// Grace period after SIGTERM before escalating to SIGKILL.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// Poll interval while waiting for a process to exit after SIGTERM.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

// ── Error ───────────────────────────────────────────────────────────────

/// Supervisor error.  No `std::error::Error` impl — dodges
/// `missing_trait_methods` on unstable `provide()`.
#[derive(Debug)]
pub enum Error {
    /// Requested binary is not on the allow-list after canonicalisation.
    NotAllowed {
        /// Canonical path that failed the allow-list check.
        binary: PathBuf,
    },
    /// No supervised agent with this id.
    NotFound {
        /// The id that was not found in the supervised set.
        agent_id: String,
    },
    /// Agent already tracked by the supervisor.
    AlreadySupervised {
        /// The id that is already supervised.
        agent_id: String,
    },
    /// OS-level I/O failure.
    Io {
        /// Static description of the operation that failed.
        context: &'static str,
        /// The underlying I/O error.
        source: io::Error,
    },
    /// Signal delivery failed.
    Signal {
        /// The `errno` returned by `kill(2)`.
        source: nix::errno::Errno,
    },
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotAllowed { binary } => {
                write!(f, "binary not on allow-list: {}", binary.display())
            }
            Self::NotFound { agent_id } => {
                write!(f, "agent not found: {agent_id}")
            }
            Self::AlreadySupervised { agent_id } => {
                write!(f, "agent already supervised: {agent_id}")
            }
            Self::Io { context, source } => write!(f, "{context}: {source}"),
            Self::Signal { source } => write!(f, "signal error: {source}"),
        }
    }
}

/// Convenience alias.
pub type Result<T> = core::result::Result<T, Error>;

// ── Supervised agent ────────────────────────────────────────────────────

/// A single supervised agent process.
#[derive(Debug)]
struct Supervised {
    /// Child handle — `Some` for agents we spawned, `None` for adopted.
    child: Option<Child>,
    /// OS pid of the agent process.
    pid: u32,
    /// Canonical binary path (for restart).
    binary: PathBuf,
    /// Working directory = agent's realm folder.
    folder: PathBuf,
    /// Extra CLI arguments passed at spawn (for restart).
    args: Vec<String>,
}

// ── Supervisor events ───────────────────────────────────────────────────

/// Events emitted by [`AgentSupervisor::check_liveness`].
#[derive(Debug)]
pub enum Event {
    /// A spawned agent exited on its own (reaped via `try_wait`).
    Exited {
        /// The agent that exited.
        agent_id: String,
        /// Process exit code, if one was reported.
        status: Option<i32>,
    },
    /// An adopted agent's pid is no longer alive.
    Vanished {
        /// The agent whose pid stopped responding to signal-0.
        agent_id: String,
    },
}

// ── AgentSupervisor ─────────────────────────────────────────────────────

/// Process-level lifecycle manager for a fleet of agents.
#[derive(Debug)]
pub struct AgentSupervisor {
    /// Canonicalised binary paths that may be spawned.
    allow_list: Vec<PathBuf>,
    /// Active agents keyed by `agent_id`.
    known: HashMap<String, Supervised>,
}

impl AgentSupervisor {
    /// Create a supervisor with the given allow-list.
    ///
    /// Each path is canonicalised eagerly; entries that fail to resolve are
    /// silently skipped (the binary may not exist yet on disk).
    pub fn new(allow_list: &[PathBuf]) -> Self {
        let resolved = allow_list
            .iter()
            .filter_map(|p| std::fs::canonicalize(p).ok())
            .collect();
        Self { allow_list: resolved, known: HashMap::new() }
    }

    /// Number of supervised agents.
    pub fn len(&self) -> usize {
        self.known.len()
    }

    /// Whether the supervisor tracks zero agents.
    pub fn is_empty(&self) -> bool {
        self.known.is_empty()
    }

    // ── Allow-list gate ─────────────────────────────────────────────

    /// Validate a binary path against the allow-list.
    ///
    /// Returns the canonicalised path on success.  Rejects symlinks / `..`
    /// that resolve outside the list (R2-15).
    pub(crate) fn validate_binary(&self, requested: &Path) -> Result<PathBuf> {
        let canonical =
            std::fs::canonicalize(requested).map_err(|e| Error::Io {
                context: "canonicalize binary path",
                source: e,
            })?;
        if self.allow_list.contains(&canonical) {
            Ok(canonical)
        } else {
            Err(Error::NotAllowed { binary: canonical })
        }
    }

    // ── Spawn ───────────────────────────────────────────────────────

    /// Spawn a new agent process in `folder`.
    ///
    /// The binary must resolve (via `realpath`) to an entry on the allow-list.
    /// The child runs in a **new process group** (`process_group(0)`) so it
    /// survives backend termination.  stdin/stdout/stderr are closed.
    ///
    /// Returns the agent's OS pid.
    pub fn spawn(
        &mut self,
        agent_id: String,
        binary: &Path,
        folder: &Path,
        extra_args: &[&str],
    ) -> Result<u32> {
        if self.known.contains_key(&agent_id) {
            return Err(Error::AlreadySupervised { agent_id });
        }
        let canonical = self.validate_binary(binary)?;
        let folder_canonical =
            std::fs::canonicalize(folder).map_err(|e| Error::Io {
                context: "canonicalize folder",
                source: e,
            })?;

        let child = Command::new(&canonical)
            .current_dir(&folder_canonical)
            .args(extra_args)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Io { context: "spawn agent", source: e })?;

        let pid = child.id();
        let _previous = self.known.insert(agent_id, Supervised {
            child: Some(child),
            pid,
            binary: canonical,
            folder: folder_canonical,
            args: extra_args.iter().map(|s| (*s).to_owned()).collect(),
        });
        Ok(pid)
    }

    // ── Stop ────────────────────────────────────────────────────────

    /// Stop a supervised agent.
    ///
    /// Sends SIGTERM, polls for exit up to [`STOP_GRACE`], then escalates
    /// to SIGKILL.  For spawned agents the child is reaped via `wait()`.
    pub fn stop(&mut self, agent_id: &str) -> Result<()> {
        let mut supervised = self.known.remove(agent_id).ok_or_else(|| {
            Error::NotFound { agent_id: agent_id.to_owned() }
        })?;
        let raw_pid =
            Pid::from_raw(i32::try_from(supervised.pid).unwrap_or(i32::MAX));

        // Phase 1: SIGTERM
        let _sent = send_signal(raw_pid, Signal::SIGTERM);

        // Phase 2: poll until dead or grace expires
        let deadline = Instant::now() + STOP_GRACE;
        let died = loop {
            if !pid_alive(raw_pid) {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            thread::sleep(POLL_INTERVAL);
        };

        // Phase 3: SIGKILL if still alive
        if !died {
            let _sent = send_signal(raw_pid, Signal::SIGKILL);
        }

        // Reap zombie if we spawned it
        if let Some(ref mut child) = supervised.child {
            let _status = child.wait();
        }
        Ok(())
    }

    // ── Restart ─────────────────────────────────────────────────────

    /// Restart a supervised agent: stop, then re-spawn with the same config.
    ///
    /// Returns the new pid.
    pub fn restart(&mut self, agent_id: &str) -> Result<u32> {
        let info = self.known.get(agent_id).ok_or_else(|| {
            Error::NotFound { agent_id: agent_id.to_owned() }
        })?;
        let binary = info.binary.clone();
        let folder = info.folder.clone();
        let args = info.args.clone();

        self.stop(agent_id)?;

        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.spawn(agent_id.to_owned(), &binary, &folder, &arg_refs)
    }

    // ── Adopt ───────────────────────────────────────────────────────

    /// Adopt a still-live agent discovered by the registry (D7).
    ///
    /// No process is spawned — the supervisor merely records the mapping so
    /// that subsequent `stop`/`restart` calls work.
    pub fn adopt(
        &mut self,
        agent_id: String,
        entry: &Entry,
        binary: PathBuf,
    ) -> Result<()> {
        if self.known.contains_key(&agent_id) {
            return Err(Error::AlreadySupervised { agent_id });
        }
        let _previous = self.known.insert(agent_id, Supervised {
            child: None,
            pid: entry.pid,
            binary,
            folder: PathBuf::from(&entry.folder),
            args: Vec::new(),
        });
        Ok(())
    }

    // ── Liveness check ──────────────────────────────────────────────

    /// Poll supervised agents and emit events for those that have exited.
    ///
    /// For **spawned** agents, `try_wait` reaps the zombie.  For **adopted**
    /// agents, `kill(pid, 0)` probes liveness.  Dead agents are removed.
    pub fn check_liveness(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let mut dead_ids = Vec::new();

        for (id, sup) in &mut self.known {
            let raw_pid =
                Pid::from_raw(i32::try_from(sup.pid).unwrap_or(i32::MAX));

            if let Some(ref mut child) = sup.child {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        events.push(Event::Exited {
                            agent_id: id.clone(),
                            status: status.code(),
                        });
                        dead_ids.push(id.clone());
                    }
                    Ok(None) => {}
                    Err(_) => {
                        events.push(Event::Vanished {
                            agent_id: id.clone(),
                        });
                        dead_ids.push(id.clone());
                    }
                }
            } else if !pid_alive(raw_pid) {
                events.push(Event::Vanished { agent_id: id.clone() });
                dead_ids.push(id.clone());
            }
        }

        for id in &dead_ids {
            let _removed = self.known.remove(id);
        }
        events
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Send a signal, swallowing ESRCH (already dead).
fn send_signal(pid: Pid, sig: Signal) -> bool {
    match signal::kill(pid, sig) {
        Ok(()) | Err(nix::errno::Errno::ESRCH) => true,
        Err(_) => false,
    }
}

/// Probe whether a pid is alive via signal-0.
pub(crate) fn pid_alive(pid: Pid) -> bool {
    matches!(
        signal::kill(pid, None),
        Ok(()) | Err(nix::errno::Errno::EPERM)
    )
}
