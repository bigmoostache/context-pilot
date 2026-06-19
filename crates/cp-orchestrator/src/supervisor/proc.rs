//! Supervised-process representation + the pty spawn helper.
//!
//! Split out of [`supervisor`](super) for the 500-line file budget. Holds the
//! [`Proc`] handle enum, the [`Supervised`] record, and [`spawn_pty_proc`] —
//! the openpty / spawn / drain dance that produces a [`Proc::Pty`].

use std::io::Read as _;
use std::path::Path;
use std::process::Child;
use std::path::PathBuf;
use std::thread;

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

use super::{Error, Result};

/// The OS-process handle behind a supervised agent.
///
/// Three flavours so the supervisor can manage agents spawned through either
/// path **and** agents adopted (not spawned) on a backend restart, with one
/// uniform `stop` / `check_liveness` surface:
///
/// * [`Std`](Self::Std) — a plain [`Child`] spawned with null stdio (the
///   original path; used by everything that does not need a terminal).
/// * [`Pty`](Self::Pty) — a `cp` TUI agent spawned attached to a pseudo-terminal
///   (it calls `enable_raw_mode()` and needs a real tty). We hold both the
///   boxed [`portable_pty::Child`] (to reap it) and the [`MasterPty`] (to keep
///   the pty open for the agent's lifetime; dropping it ends the drain thread).
/// * [`Adopted`](Self::Adopted) — a still-live agent discovered in the registry
///   on backend restart; we never owned its `Child`, so liveness is probed by
///   `kill(pid, 0)` and there is nothing to reap.
pub(super) enum Proc {
    /// A plain child spawned with null stdio.
    Std(Child),
    /// A `cp` TUI agent spawned on a pseudo-terminal.
    Pty {
        /// The boxed pty child — reaped on stop, polled for liveness.
        child: Box<dyn portable_pty::Child + Send + Sync>,
        /// The master side of the pty, held so the agent's tty stays open for
        /// its whole lifetime; dropping it signals EOF to the drain thread.
        _master: Box<dyn MasterPty + Send>,
    },
    /// An adopted agent we did not spawn — nothing to reap.
    Adopted,
}

/// A single supervised agent process.
pub(super) struct Supervised {
    /// The OS-process handle (spawned-std / spawned-pty / adopted).
    pub(super) proc: Proc,
    /// OS pid of the agent process.
    pub(super) pid: u32,
    /// Canonical binary path (for restart).
    pub(super) binary: PathBuf,
    /// Working directory = agent's realm folder.
    pub(super) folder: PathBuf,
    /// Extra CLI arguments passed at spawn (for restart).
    pub(super) args: Vec<String>,
}

// Manual Debug — `Box<dyn Child>` / `Box<dyn MasterPty>` are not `Debug`.
impl core::fmt::Debug for Supervised {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let kind = match self.proc {
            Proc::Std(_) => "std",
            Proc::Pty { .. } => "pty",
            Proc::Adopted => "adopted",
        };
        f.debug_struct("Supervised")
            .field("proc", &kind)
            .field("pid", &self.pid)
            .field("binary", &self.binary)
            .field("folder", &self.folder)
            .field("args", &self.args)
            .finish()
    }
}

/// Open a pty, spawn `binary` attached to its slave in `folder`, and spin a
/// detached thread draining the master. Returns the [`Proc::Pty`] handle (master
/// retained) plus the agent's OS pid.
///
/// The caller has already validated `binary` against the allow-list and
/// canonicalised both paths. `env` is layered on top of the inherited
/// environment, so the agent keeps the orchestrator's credentials while
/// receiving the caller's `CP_BRIDGE` / `CP_AGENTS_DIR` overrides.
///
/// # Errors
///
/// [`Error::Pty`] for any pty-layer failure (openpty / spawn / reader clone).
pub(super) fn spawn_pty_proc(
    binary: &Path,
    folder: &Path,
    env: &[(&str, &str)],
) -> Result<(Proc, u32)> {
    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize { rows: 40, cols: 120, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| Error::Pty { detail: e.to_string() })?;

    let mut cmd = CommandBuilder::new(binary);
    cmd.cwd(folder);
    // Inherit the orchestrator's environment (API keys, PATH, …) so the agent
    // boots with the same credentials, then layer the caller's overrides
    // (CP_BRIDGE, CP_AGENTS_DIR) on top.
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }
    for (k, v) in env {
        cmd.env(*k, *v);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| Error::Pty { detail: e.to_string() })?;
    // The parent must not retain the slave (the child owns it now); holding it
    // open would prevent the master ever seeing EOF.
    drop(pair.slave);

    let pid = child.process_id().unwrap_or(0);

    // Drain the master into the void so the agent's tty writes never block on a
    // full buffer. The thread ends when the master is dropped (stop) or the
    // agent exits (read returns 0 / errors).
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| Error::Pty { detail: e.to_string() })?;
    let _drain = thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
        }
    });

    Ok((Proc::Pty { child, _master: pair.master }, pid))
}
