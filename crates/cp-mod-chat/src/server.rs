//! Tuwunel homeserver process lifecycle management.
//!
//! Handles starting, stopping, and health-checking the local Matrix
//! homeserver that runs alongside Context Pilot. Each project gets
//! a unique port derived from the project path (range 6167–6667),
//! so multiple Context Pilot instances can coexist on one machine.
//!
//! The server process is tracked via a PID file, not a `Child` handle,
//! so it survives TUI reloads and can be reconnected to on restart.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cp_base::state::runtime::State;

use crate::types::{ChatState, ServerStatus};

/// Bind address — always localhost, never exposed.
const BIND_HOST: &str = "127.0.0.1";

/// Start of the port range for derived ports (inclusive).
const PORT_RANGE_START: u16 = 6167;

/// Size of the port range (6167–6667 = 501 ports).
const PORT_RANGE_SIZE: u16 = 501;

/// Maximum time to wait for the server to become healthy after start.
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(15);

/// Interval between health check retries during startup.
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_millis(500);

/// Grace period before force-killing after requesting termination.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Path to the Tuwunel binary relative to the user's home directory.
const BIN_REL_PATH: &str = ".context-pilot/bin/tuwunel";

/// Returns the absolute path to the Tuwunel binary.
#[must_use]
pub(crate) fn binary_path() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(BIN_REL_PATH))
}

/// Returns the matrix data directory inside the project's `.context-pilot/`.
#[must_use]
pub(crate) fn data_dir(project_root: &Path) -> PathBuf {
    project_root.join(".context-pilot/matrix")
}

/// Returns the path to the homeserver config file.
#[must_use]
pub(crate) fn config_path(project_root: &Path) -> PathBuf {
    data_dir(project_root).join("homeserver.toml")
}

/// Returns the path to the PID file.
#[must_use]
pub(crate) fn pid_path(project_root: &Path) -> PathBuf {
    data_dir(project_root).join("tuwunel.pid")
}

// -- Dynamic port derivation -------------------------------------------------

/// Derive a deterministic port from the project's absolute path.
///
/// Hashes the canonical working directory and maps it into the range
/// `6167..=6667`. Two different project directories will (almost
/// certainly) get different ports, allowing concurrent instances.
#[must_use]
pub(crate) fn derive_port() -> u16 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hash as _;

    let mut hasher = DefaultHasher::new();
    let cwd = std::env::current_dir().unwrap_or_default();
    "tuwunel_port_salt".hash(&mut hasher);
    cwd.hash(&mut hasher);
    let h = std::hash::Hasher::finish(&hasher);
    // PORT_RANGE_SIZE is 501, so remainder always fits in u16
    let range = u64::from(PORT_RANGE_SIZE);
    let offset = h.checked_rem(range).unwrap_or(0);
    let offset_u16 = u16::try_from(offset).unwrap_or(0);
    PORT_RANGE_START.wrapping_add(offset_u16)
}

/// Read the port number from `homeserver.toml`.
///
/// Parses the `port = [N]` line. Falls back to [`derive_port`] if the
/// config cannot be read or the port line is absent.
#[must_use]
pub(crate) fn read_port(project_root: &Path) -> u16 {
    let cfg = config_path(project_root);
    let Ok(content) = std::fs::read_to_string(cfg) else {
        return derive_port();
    };
    // Parse the `port = [6167]` line — simple and robust
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("port")
            && let Some(bracket_start) = trimmed.find('[')
            && let Some(bracket_end) = trimmed.find(']')
            && bracket_end > bracket_start
        {
            let inner = trimmed.get(bracket_start.saturating_add(1)..bracket_end).unwrap_or("");
            if let Ok(p) = inner.trim().parse::<u16>() {
                return p;
            }
        }
    }
    derive_port()
}

/// Returns the full `host:port` address string for the homeserver.
///
/// Reads the port from the config file (or derives it if the config
/// doesn't exist yet). This replaces the old `SERVER_ADDR` constant.
#[must_use]
pub(crate) fn server_addr() -> String {
    let port = read_port(Path::new("."));
    format!("{BIND_HOST}:{port}")
}

// -- PID file management -----------------------------------------------------

/// Write the server PID to the PID file.
fn write_pid(project_root: &Path, pid: u32) -> Result<(), String> {
    let path = pid_path(project_root);
    std::fs::write(&path, pid.to_string()).map_err(|e| format!("Cannot write PID file {}: {e}", path.display()))
}

/// Read the PID from the PID file (if it exists).
fn read_pid(project_root: &Path) -> Option<u32> {
    let path = pid_path(project_root);
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Remove the PID file.
fn remove_pid(project_root: &Path) {
    let path = pid_path(project_root);
    let _r = std::fs::remove_file(path);
}

/// Check if a process with the given PID is alive.
///
/// Uses `kill -0` which checks existence without sending a signal.
fn is_pid_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

// -- Server lifecycle --------------------------------------------------------

/// Start the Tuwunel homeserver, reusing an existing process if found.
///
/// **Orphan recovery**: checks the PID file first. If a process is
/// alive AND the health endpoint responds, the existing server is
/// reused — no new process is spawned. This makes TUI reloads seamless.
///
/// **Fresh start**: validates prerequisites, spawns the binary, writes
/// the PID file, and polls the health endpoint until ready (up to 15 s).
///
/// # Errors
///
/// Returns a descriptive error if the binary is missing, config is
/// absent, the process fails to spawn, or health check times out.
pub(crate) fn start_server(state: &mut State) -> Result<(), String> {
    let root = Path::new(".");

    // ── Phase 0: try to reconnect to an existing server ────────────
    if let Some(pid) = read_pid(root) {
        if is_pid_alive(pid) && health_check().is_ok() {
            // Server is alive and healthy — reuse it
            let cs = ChatState::get_mut(state);
            cs.server_pid = Some(pid);
            cs.server_status = ServerStatus::Running;
            log::info!("Reconnected to existing Tuwunel server (PID {pid})");
            // Still run post-start setup (idempotent)
            if let Err(e) = crate::bootstrap::post_start_setup(state) {
                log::warn!("Post-start setup incomplete: {e}");
            }
            return Ok(());
        }
        // PID file is stale — clean it up
        remove_pid(root);
    }

    // ── Phase 1: validate prerequisites and spawn ──────────────────
    {
        let cs = ChatState::get_mut(state);

        if cs.server_status == ServerStatus::Running {
            return Ok(());
        }

        cs.server_status = ServerStatus::Starting;
    }

    let bin = binary_path().ok_or("Cannot determine home directory for Tuwunel binary")?;
    if !bin.exists() {
        ChatState::get_mut(state).server_status = ServerStatus::Stopped;
        return Err(format!("Tuwunel binary not found at {}. Install it first.", bin.display()));
    }

    let data = data_dir(root);
    let cfg = config_path(root);
    if !cfg.exists() {
        ChatState::get_mut(state).server_status = ServerStatus::Stopped;
        return Err(format!("Config not found at {}. Run bootstrap first.", cfg.display()));
    }

    let log_path = data.join("server.log");
    let log_file = std::fs::File::create(&log_path).map_err(|e| {
        ChatState::get_mut(state).server_status = ServerStatus::Error(e.to_string());
        format!("Cannot create server log at {}: {e}", log_path.display())
    })?;
    let log_err = log_file.try_clone().map_err(|e| {
        ChatState::get_mut(state).server_status = ServerStatus::Error(e.to_string());
        format!("Cannot duplicate log file handle: {e}")
    })?;

    let child = Command::new(&bin)
        .arg("--config")
        .arg(&cfg)
        .stdin(Stdio::null())
        .stdout(log_file)
        .stderr(log_err)
        .spawn()
        .map_err(|e| {
            ChatState::get_mut(state).server_status = ServerStatus::Error(e.to_string());
            format!("Failed to spawn Tuwunel: {e}")
        })?;

    let pid = child.id();
    ChatState::get_mut(state).server_pid = Some(pid);

    // Write PID file so we can find this process after a reload
    if let Err(e) = write_pid(root, pid) {
        log::warn!("Failed to write PID file: {e}");
    }

    // ── Phase 2: wait for health and run post-start setup ──────────
    let health = wait_for_health();

    match health {
        Ok(()) => {
            ChatState::get_mut(state).server_status = ServerStatus::Running;
        }
        Err(ref e) => {
            ChatState::get_mut(state).server_status = ServerStatus::Error(e.clone());
            return Err(e.clone());
        }
    }

    if let Err(e) = crate::bootstrap::post_start_setup(state) {
        log::warn!("Post-start setup incomplete: {e}");
    }

    Ok(())
}

/// Stop the Tuwunel homeserver gracefully.
///
/// Uses the PID from `ChatState` (or the PID file as fallback).
/// Sends SIGTERM, waits up to 5 seconds, then force-kills.
/// Cleans up the PID file afterwards.
pub(crate) fn stop_server(state: &mut State) {
    let root = Path::new(".");
    let cs = ChatState::get_mut(state);

    // Determine which PID to kill: state first, PID file as fallback
    let pid = cs.server_pid.or_else(|| read_pid(root));

    if let Some(pid) = pid {
        // Request graceful shutdown
        let _term = Command::new("kill").arg(pid.to_string()).status();

        // Wait for exit within the grace period
        let deadline = Instant::now().checked_add(SHUTDOWN_GRACE);
        loop {
            if !is_pid_alive(pid) {
                break;
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                // Grace period expired — force kill
                let _kill = Command::new("kill").arg("-9").arg(pid.to_string()).status();
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    remove_pid(root);
    cs.server_pid = None;
    cs.server_status = ServerStatus::Stopped;
}

/// Check if the homeserver is healthy by hitting the versions endpoint.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or returns a non-`2xx` status.
pub(crate) fn health_check() -> Result<(), String> {
    let url = format!("http://{}/_matrix/client/versions", server_addr());
    let resp = reqwest::blocking::get(&url).map_err(|e| format!("Health check failed: {e}"))?;
    if resp.status().is_success() { Ok(()) } else { Err(format!("Health check returned HTTP {}", resp.status())) }
}

/// Poll the health endpoint until it responds or the timeout expires.
fn wait_for_health() -> Result<(), String> {
    let deadline = Instant::now().checked_add(HEALTH_CHECK_TIMEOUT);
    loop {
        if health_check().is_ok() {
            return Ok(());
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(format!("Tuwunel did not become healthy within {}s", HEALTH_CHECK_TIMEOUT.as_secs()));
        }
        std::thread::sleep(HEALTH_CHECK_INTERVAL);
    }
}
