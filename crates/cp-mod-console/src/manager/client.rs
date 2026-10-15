//! Client side of the console server: socket paths, request/response, spawn, orphan cleanup.

use std::collections::HashSet;
use std::fs;
use std::hash::BuildHasher;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use cp_base::config::constants;

use crate::CONSOLE_DIR;

/// Socket path for the console server.
fn server_socket_path() -> PathBuf {
    PathBuf::from(constants::STORE_DIR).join(CONSOLE_DIR).join("server.sock")
}

/// PID file for the console server.
fn server_pid_path() -> PathBuf {
    PathBuf::from(constants::STORE_DIR).join(CONSOLE_DIR).join("server.pid")
}

/// Path to the server binary. Checks multiple locations:
/// 1. Next to the current TUI binary (deployed)
/// 2. In target/release/ (cargo run --release)
/// 3. In target/debug/ (cargo run)
fn server_binary_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let next_to_exe = exe.parent().unwrap_or_else(|| std::path::Path::new(".")).join("cp-console-server");
    if next_to_exe.exists() {
        return next_to_exe;
    }

    // Try workspace target directories (when running via cargo run)
    // Walk up from exe to find the workspace root (has Cargo.toml)
    let mut dir = exe.parent();
    while let Some(d) = dir {
        let cargo_toml = d.join("Cargo.toml");
        if cargo_toml.exists() {
            // Check target/release and target/debug
            for profile in &["release", "debug"] {
                let candidate = d.join("target").join(profile).join("cp-console-server");
                if candidate.exists() {
                    return candidate;
                }
            }
        }
        dir = d.parent();
    }

    // Fallback to next-to-exe (will fail with a clear error)
    next_to_exe
}

/// Build the log file path for a given session key (always absolute).
#[must_use]
pub fn log_file_path(key: &str) -> PathBuf {
    let base = PathBuf::from(constants::STORE_DIR).join(CONSOLE_DIR).join(format!("{key}.log"));
    if base.is_absolute() { base } else { constants::project_root().join(base) }
}

// ---------------------------------------------------------------------------
// Server client
// ---------------------------------------------------------------------------

/// Send a JSON command to the server and read the response.
///
/// # Errors
///
/// Returns `Err` if the socket connection, write, read, or JSON parse fails,
/// or if the server response indicates an error.
pub(crate) fn server_request(req: &serde_json::Value) -> Result<serde_json::Value, String> {
    let sock_path = server_socket_path();
    let stream = {
        let _p = cp_base::perf_span!("console_connect");
        UnixStream::connect(&sock_path).map_err(|e| format!("Failed to connect to console server: {e}"))?
    };
    let _: Option<()> = stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok();
    let _: Option<()> = stream.set_write_timeout(Some(std::time::Duration::from_secs(5))).ok();

    let mut writer = stream.try_clone().map_err(|e| format!("Clone failed: {e}"))?;
    let reader = BufReader::new(stream);

    {
        let _p = cp_base::perf_span!("console_send");
        let mut line = serde_json::to_string(req).map_err(|e| format!("Serialize failed: {e}"))?;
        line.push('\n');
        writer.write_all(line.as_bytes()).map_err(|e| format!("Write failed: {e}"))?;
        writer.flush().map_err(|e| format!("Flush failed: {e}"))?;
    }

    let mut resp_line = String::new();
    let mut buf_reader = reader;
    {
        // Blocks until the server has done the work (spawn, write stdin, …).
        let _p = cp_base::perf_span!("console_reply");
        let _: usize = buf_reader.read_line(&mut resp_line).map_err(|e| format!("Read failed: {e}"))?;
    }

    let resp: serde_json::Value = {
        let _p = cp_base::perf_span!("console_parse");
        serde_json::from_str(resp_line.trim()).map_err(|e| format!("Parse response failed: {e}"))?
    };

    if resp.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        Ok(resp)
    } else {
        let err = resp.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error");
        Err(err.to_owned())
    }
}

/// Find the running server or spawn a new one.
///
/// # Errors
///
/// Returns `Err` if the console directory cannot be created, or if the
/// server binary fails to spawn.
pub fn find_or_create_server() -> Result<(), String> {
    // Ensure console directory exists
    let console_dir = PathBuf::from(constants::STORE_DIR).join(CONSOLE_DIR);
    fs::create_dir_all(&console_dir).map_err(|e| format!("Failed to create console dir: {e}"))?;

    // Try connecting to existing server
    let ping = serde_json::json!({"cmd": "ping"});
    if server_request(&ping).is_ok() {
        return Ok(()); // Server already running
    }

    // Server not running — spawn it
    let binary = server_binary_path();
    if !binary.exists() {
        return Err(format!("Console server binary not found at {}", binary.display()));
    }

    let sock_path = server_socket_path();
    let sock_str = sock_path.to_string_lossy().to_string();

    // Remove stale socket/pid files
    let _: Option<()> = fs::remove_file(&sock_path).ok();
    let _: Option<()> = fs::remove_file(server_pid_path()).ok();

    let mut cmd = Command::new(&binary);
    let _: &mut Command = cmd.arg(&sock_str);
    let _: &mut Command = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());

    drop(cmd.spawn().map_err(|e| format!("Failed to spawn console server: {e}"))?);

    // Wait for socket to appear — geometric backoff (25ms → 500ms cap, 3s budget)
    let mut interval = std::time::Duration::from_millis(25);
    let max_interval = std::time::Duration::from_millis(500);
    let deadline = std::time::Instant::now().checked_add(std::time::Duration::from_secs(3));
    loop {
        std::thread::sleep(interval);
        if server_request(&ping).is_ok() {
            return Ok(());
        }
        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            break;
        }
        interval = interval.saturating_mul(2).min(max_interval);
    }

    Err("Console server failed to start within 3 seconds".to_owned())
}

/// Kill orphaned processes: server sessions owned by one of `owners` (agents
/// loaded in this process) whose key is not in `known_keys`.
///
/// The console server is shared by every cpilot process on the project, so a
/// session owned by an agent this process did not load is never touched, and
/// keys with no recognisable owner are left alone.
pub fn kill_orphaned_processes<S, T>(known_keys: &HashSet<String, S>, owners: &HashSet<String, T>)
where
    S: BuildHasher,
    T: BuildHasher,
{
    let list = serde_json::json!({"cmd": "list"});
    if let Ok(resp) = server_request(&list)
        && let Some(sessions) = resp.get("sessions").and_then(|v| v.as_array())
    {
        for session in sessions {
            if let Some(key) = session.get("key").and_then(|v| v.as_str())
                && is_orphan(key, known_keys, owners)
            {
                // Orphan — remove it from server (kills process)
                let remove = serde_json::json!({"cmd": "remove", "key": key});
                drop(server_request(&remove).ok());
            }
        }
    }
}

/// Owner agent of a session key `{prefix}_{agent}_{n}` (see `ConsoleState::alloc_key`).
#[must_use]
pub fn owner_of(key: &str) -> Option<&str> {
    let (_, rest) = key.split_once('_')?;
    let (agent, n) = rest.rsplit_once('_')?;
    (!agent.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then_some(agent)
}

/// Whether `key` is an orphan this process may kill.
fn is_orphan<S, T>(key: &str, known_keys: &HashSet<String, S>, owners: &HashSet<String, T>) -> bool
where
    S: BuildHasher,
    T: BuildHasher,
{
    !known_keys.contains(key) && owner_of(key).is_some_and(|o| owners.contains(o))
}
