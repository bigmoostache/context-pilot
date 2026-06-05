//! Session directory management for headless daemon instances.
//!
//! Each project gets a session directory at `~/.context-pilot/sessions/<path-hash>/`
//! containing the Unix socket, PID file, and project path for discovery.
//! See design doc §9.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SESSIONS_DIR: &str = "sessions";
const SOCKET_NAME: &str = "daemon.sock";
const PID_NAME: &str = "daemon.pid";
const PROJECT_PATH_NAME: &str = "project_path";
const LOG_NAME: &str = "daemon.log";

/// Info about a running daemon session, returned by [`list_sessions`].
#[derive(Debug)]
pub(crate) struct SessionInfo {
    /// Canonical filesystem path of the project.
    pub project_path: String,
    /// OS process ID of the daemon.
    pub pid: u32,
    /// Session directory containing socket/PID/log files.
    pub session_dir: PathBuf,
}

// ── Path helpers ─────────────────────────────────────────────────

/// Base directory for all sessions: `~/.context-pilot/sessions/`.
fn sessions_base_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".context-pilot").join(SESSIONS_DIR)
}

/// Stable FNV-1a hash of a byte slice, returned as a 16-char hex string.
///
/// Used to derive a filesystem-safe directory name from a project path.
/// Deterministic across runs and Rust versions (unlike `DefaultHasher`).
fn path_hash(data: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// Session directory for a project: `~/.context-pilot/sessions/<hash>/`.
///
/// Callers should pass a **canonicalized** path (via `fs::canonicalize`)
/// to ensure consistent hashing regardless of symlinks or trailing slashes.
pub(crate) fn session_dir(project_path: &Path) -> PathBuf {
    let canonical = project_path.to_string_lossy();
    sessions_base_dir().join(path_hash(canonical.as_bytes()))
}

/// Unix socket path for a project's daemon.
pub(crate) fn socket_path(project_path: &Path) -> PathBuf {
    session_dir(project_path).join(SOCKET_NAME)
}

/// PID file path for a project's daemon.
fn pid_file_path(project_path: &Path) -> PathBuf {
    session_dir(project_path).join(PID_NAME)
}

/// Daemon log file path (stdout/stderr redirect).
pub(crate) fn log_path(project_path: &Path) -> PathBuf {
    session_dir(project_path).join(LOG_NAME)
}

// ── PID management ───────────────────────────────────────────────

/// Write PID and project path to the session directory. Creates the
/// directory tree if it doesn't exist.
pub(crate) fn write_session_files(project_path: &Path, pid: u32) -> std::io::Result<()> {
    let dir = session_dir(project_path);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(PID_NAME), pid.to_string())?;
    fs::write(dir.join(PROJECT_PATH_NAME), project_path.to_string_lossy().as_bytes())?;
    Ok(())
}

/// Read the daemon PID from a project's session directory.
///
/// Returns `None` if the file doesn't exist or can't be parsed.
pub(crate) fn read_pid(project_path: &Path) -> Option<u32> {
    fs::read_to_string(pid_file_path(project_path)).ok()?.trim().parse().ok()
}

/// Non-blocking signal-0 probe — checks if a process is alive without
/// sending an actual signal. Works on Linux and macOS.
pub(crate) fn is_pid_alive(pid: u32) -> bool {
    // Same pattern as cp-console-server/src/main.rs
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Check whether a daemon is currently running for this project.
pub(crate) fn is_daemon_running(project_path: &Path) -> bool {
    read_pid(project_path).is_some_and(|pid| is_pid_alive(pid))
}

// ── Cleanup ──────────────────────────────────────────────────────

/// Remove stale session files for a dead daemon. Returns `true` if
/// a stale session was found and cleaned up.
///
/// Keeps `daemon.log` for post-mortem debugging.
pub(crate) fn cleanup_stale_session(project_path: &Path) -> bool {
    let Some(pid) = read_pid(project_path) else {
        return false;
    };
    if is_pid_alive(pid) {
        return false;
    }

    let dir = session_dir(project_path);
    drop(fs::remove_file(dir.join(SOCKET_NAME)));
    drop(fs::remove_file(dir.join(PID_NAME)));
    drop(fs::remove_file(dir.join(PROJECT_PATH_NAME)));
    true
}

/// Remove all session files including the directory itself.
/// Called after a graceful daemon shutdown.
pub(crate) fn remove_session(project_path: &Path) {
    let dir = session_dir(project_path);
    drop(fs::remove_file(dir.join(SOCKET_NAME)));
    drop(fs::remove_file(dir.join(PID_NAME)));
    drop(fs::remove_file(dir.join(PROJECT_PATH_NAME)));
    drop(fs::remove_file(dir.join(LOG_NAME)));
    drop(fs::remove_dir(&dir));
}

// ── Discovery ────────────────────────────────────────────────────

/// Scan all session directories and return info for live daemons.
///
/// Stale sessions (dead PIDs) are cleaned up automatically during the scan.
pub(crate) fn list_sessions() -> Vec<SessionInfo> {
    let base = sessions_base_dir();
    let entries = match fs::read_dir(&base) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut sessions = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }

        let project_path = match fs::read_to_string(dir.join(PROJECT_PATH_NAME)) {
            Ok(p) => p.trim().to_owned(),
            Err(_) => continue,
        };

        let Some(pid) = fs::read_to_string(dir.join(PID_NAME)).ok().and_then(|s| s.trim().parse::<u32>().ok()) else {
            continue;
        };

        if is_pid_alive(pid) {
            sessions.push(SessionInfo { project_path, pid, session_dir: dir });
        } else {
            // Dead daemon — clean up stale files (keep log)
            drop(fs::remove_file(dir.join(SOCKET_NAME)));
            drop(fs::remove_file(dir.join(PID_NAME)));
            drop(fs::remove_file(dir.join(PROJECT_PATH_NAME)));
        }
    }

    sessions
}
