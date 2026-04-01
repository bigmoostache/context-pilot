//! Global Tuwunel homeserver process lifecycle.
//!
//! The Matrix homeserver is a **shared, machine-wide resource** that
//! lives under `~/.context-pilot/matrix/`. Every Context Pilot instance
//! on the machine connects to the same server as a separate Matrix user.
//!
//! Communication happens exclusively over a **Unix domain socket** —
//! no TCP ports are opened, nothing touches the network.
//!
//! The first cpilot to need chat starts the server; subsequent instances
//! discover it via the PID file and reuse it. No instance ever stops
//! the server on shutdown — it runs until reboot or manual kill.

use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cp_base::state::runtime::State;

use crate::types::{ChatState, ServerStatus};

// -- Timeouts ----------------------------------------------------------------

/// Maximum time to wait for the server to become healthy after start.
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(15);

/// Interval between health check retries during startup.
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_millis(500);

// -- Global paths ------------------------------------------------------------

/// Root of all global Matrix data: `~/.context-pilot/matrix/`.
#[must_use]
pub(crate) fn global_matrix_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".context-pilot/matrix"))
}

/// Homeserver config: `~/.context-pilot/matrix/homeserver.toml`.
#[must_use]
pub(crate) fn global_config_path() -> Option<PathBuf> {
    global_matrix_dir().map(|d| d.join("homeserver.toml"))
}

/// PID file: `~/.context-pilot/matrix/tuwunel.pid`.
#[must_use]
pub(crate) fn global_pid_path() -> Option<PathBuf> {
    global_matrix_dir().map(|d| d.join("tuwunel.pid"))
}

/// Unix domain socket: `~/.context-pilot/matrix/tuwunel.sock`.
#[must_use]
pub(crate) fn global_socket_path() -> Option<PathBuf> {
    global_matrix_dir().map(|d| d.join("tuwunel.sock"))
}

/// Server log file: `~/.context-pilot/matrix/server.log`.
#[must_use]
pub(crate) fn global_log_path() -> Option<PathBuf> {
    global_matrix_dir().map(|d| d.join("server.log"))
}

/// Path to the Tuwunel binary: `~/.context-pilot/bin/tuwunel`.
#[must_use]
pub(crate) fn binary_path() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".context-pilot/bin/tuwunel"))
}

// -- PID file management -----------------------------------------------------

/// Write the server PID to the global PID file.
fn write_pid(pid: u32) -> Result<(), String> {
    let path = global_pid_path().ok_or("Cannot determine home directory")?;
    std::fs::write(&path, pid.to_string()).map_err(|e| format!("Cannot write PID file {}: {e}", path.display()))
}

/// Read the PID from the global PID file (if it exists).
fn read_pid() -> Option<u32> {
    let path = global_pid_path()?;
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Remove the global PID file.
fn remove_pid() {
    if let Some(path) = global_pid_path() {
        let _r = std::fs::remove_file(path);
    }
}

/// Check if a process with the given PID is alive (not a zombie).
///
/// Reads `/proc/<pid>/status` on Linux to exclude zombies, falling
/// back to `kill -0` on other platforms.
fn is_pid_alive(pid: u32) -> bool {
    let proc_status = format!("/proc/{pid}/status");
    if let Ok(content) = std::fs::read_to_string(&proc_status) {
        for line in content.lines() {
            if let Some(state) = line.strip_prefix("State:") {
                let trimmed = state.trim();
                return !trimmed.starts_with('Z') && !trimmed.starts_with('X');
            }
        }
    }
    // Fallback: kill -0
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

// -- UDS HTTP helpers --------------------------------------------------------

/// Decode an HTTP chunked transfer-encoded body.
///
/// Format: `<hex-size>\r\n<data>\r\n` repeated, terminated by `0\r\n\r\n`.
/// Returns the reassembled body. If the input doesn't look chunked
/// (no valid hex prefix), returns it unchanged as a fallback.
fn decode_chunked(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut remaining = raw;

    loop {
        let Some(size_end) = remaining.find("\r\n") else {
            return if result.is_empty() { raw.to_string() } else { result };
        };

        let size_str = remaining.get(..size_end).unwrap_or("");
        let chunk_size = match usize::from_str_radix(size_str.trim(), 16) {
            Ok(0) => return result, // Terminal chunk
            Ok(n) => n,
            Err(_) => return if result.is_empty() { raw.to_string() } else { result },
        };

        // Skip past "<size>\r\n"
        let data_start = size_end.saturating_add(2);
        let data_end = data_start.saturating_add(chunk_size);

        if data_end > remaining.len() {
            if let Some(data) = remaining.get(data_start..) {
                result.push_str(data);
            }
            return result;
        }

        if let Some(chunk) = remaining.get(data_start..data_end) {
            result.push_str(chunk);
        }

        // Skip past data + \r\n
        remaining = remaining.get(data_end.saturating_add(2)..).unwrap_or("");
    }
}

/// Extract the HTTP response body, handling chunked transfer encoding.
///
/// Splits headers from body at the first `\r\n\r\n` boundary, then
/// decodes chunked encoding if the `transfer-encoding: chunked` header
/// is present.
pub(crate) fn extract_body(response: &str) -> String {
    let (headers, raw_body) = response.split_once("\r\n\r\n").unwrap_or((response, ""));

    let is_chunked = headers.lines().any(|line| {
        line.to_ascii_lowercase().starts_with("transfer-encoding:") && line.to_ascii_lowercase().contains("chunked")
    });

    if is_chunked { decode_chunked(raw_body) } else { raw_body.to_string() }
}

/// Send a raw HTTP/1.1 GET request over a Unix domain socket.
///
/// Returns the response body on success (HTTP 2xx), or an error
/// description otherwise. This avoids pulling in `reqwest` for
/// simple health checks over UDS.
fn uds_get(path: &str) -> Result<String, String> {
    let sock = global_socket_path().ok_or("Cannot determine socket path")?;
    let mut stream = UnixStream::connect(&sock).map_err(|e| format!("UDS connect failed: {e}"))?;
    let _r = stream.set_read_timeout(Some(Duration::from_secs(5)));

    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).map_err(|e| format!("UDS write failed: {e}"))?;

    let mut response = String::with_capacity(1024);
    let _n = stream.read_to_string(&mut response).map_err(|e| format!("UDS read failed: {e}"))?;

    // Parse status line
    let status_line = response.lines().next().unwrap_or("");
    let status_code = status_line.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()).unwrap_or(0);

    if (200..300).contains(&status_code) {
        Ok(extract_body(&response))
    } else {
        Err(format!("HTTP {status_code}: {status_line}"))
    }
}

/// Send a raw HTTP/1.1 POST request with JSON body over UDS.
///
/// Used for account registration and other direct API calls that
/// bypass the matrix-sdk.
pub(crate) fn uds_post(path: &str, json_body: &str) -> Result<(u16, String), String> {
    let sock = global_socket_path().ok_or("Cannot determine socket path")?;
    let mut stream = UnixStream::connect(&sock).map_err(|e| format!("UDS connect failed: {e}"))?;
    let _r = stream.set_read_timeout(Some(Duration::from_secs(10)));

    let content_len = json_body.len();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {content_len}\r\nConnection: close\r\n\r\n{json_body}"
    );
    stream.write_all(request.as_bytes()).map_err(|e| format!("UDS write failed: {e}"))?;

    let mut response = Vec::with_capacity(4096);
    let _n = stream.read_to_end(&mut response).map_err(|e| format!("UDS read failed: {e}"))?;
    let response_str = String::from_utf8_lossy(&response);

    let status_line = response_str.lines().next().unwrap_or("");
    let status_code = status_line.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()).unwrap_or(0);

    let body = extract_body(&response_str);
    Ok((status_code, body))
}

/// Send a raw HTTP/1.1 PUT request with JSON body over UDS.
///
/// Used for profile updates and other PUT-based Matrix API calls.
pub(crate) fn uds_put(path: &str, json_body: &str, access_token: &str) -> Result<(u16, String), String> {
    let sock = global_socket_path().ok_or("Cannot determine socket path")?;
    let mut stream = UnixStream::connect(&sock).map_err(|e| format!("UDS connect failed: {e}"))?;
    let _r = stream.set_read_timeout(Some(Duration::from_secs(10)));

    let content_len = json_body.len();
    let request = format!(
        "PUT {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {content_len}\r\nAuthorization: Bearer {access_token}\r\nConnection: close\r\n\r\n{json_body}"
    );
    stream.write_all(request.as_bytes()).map_err(|e| format!("UDS write failed: {e}"))?;

    let mut response = Vec::with_capacity(4096);
    let _n = stream.read_to_end(&mut response).map_err(|e| format!("UDS read failed: {e}"))?;
    let response_str = String::from_utf8_lossy(&response);

    let status_line = response_str.lines().next().unwrap_or("");
    let status_code = status_line.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()).unwrap_or(0);

    let body = extract_body(&response_str);
    Ok((status_code, body))
}

// -- Server lifecycle --------------------------------------------------------

/// Start the global Tuwunel homeserver, reusing an existing process.
///
/// **Orphan recovery**: checks the global PID file first. If a process
/// is alive AND the health endpoint responds over UDS, the existing
/// server is reused — no new process spawned.
///
/// **Fresh start**: validates prerequisites, creates global dirs, runs
/// bootstrap if needed, spawns the binary, writes the PID file, and
/// polls the health endpoint until ready (up to 15 s).
///
/// # Errors
///
/// Returns a descriptive error if the binary is missing, config is
/// absent, the process fails to spawn, or health check times out.
pub(crate) fn start_server(state: &mut State) -> Result<(), String> {
    // Phase 0: try to reconnect to an existing server
    if let Some(pid) = read_pid() {
        if is_pid_alive(pid) && health_check().is_ok() {
            let cs = ChatState::get_mut(state);
            cs.server_pid = Some(pid);
            cs.server_status = ServerStatus::Running;
            log::info!("Reconnected to existing Tuwunel server (PID {pid})");
            return Ok(());
        }
        // PID file is stale
        remove_pid();
    }

    // Phase 1: validate prerequisites and spawn
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

    let cfg = global_config_path().ok_or("Cannot determine global config path")?;
    if !cfg.exists() {
        ChatState::get_mut(state).server_status = ServerStatus::Stopped;
        return Err(format!("Config not found at {}. Run bootstrap first.", cfg.display()));
    }

    let log_path = global_log_path().ok_or("Cannot determine global log path")?;
    let log_file = std::fs::File::create(&log_path).map_err(|e| {
        ChatState::get_mut(state).server_status = ServerStatus::Error(e.to_string());
        format!("Cannot create server log at {}: {e}", log_path.display())
    })?;
    let log_err = log_file.try_clone().map_err(|e| {
        ChatState::get_mut(state).server_status = ServerStatus::Error(e.to_string());
        format!("Cannot duplicate log file handle: {e}")
    })?;

    // Build the command with --execute for appservice registrations
    let mut cmd = Command::new(&bin);
    {
        let _r = cmd.arg("--config").arg(&cfg);
    }

    // Here be dragons: Tuwunel only accepts --execute at startup
    for reg_yaml in crate::bridges::build_appservice_execute_args() {
        let _r = cmd.arg("--execute").arg(reg_yaml);
    }

    let child = cmd.stdin(Stdio::null()).stdout(log_file).stderr(log_err).spawn().map_err(|e| {
        ChatState::get_mut(state).server_status = ServerStatus::Error(e.to_string());
        format!("Failed to spawn Tuwunel: {e}")
    })?;

    let pid = child.id();
    ChatState::get_mut(state).server_pid = Some(pid);

    if let Err(e) = write_pid(pid) {
        log::warn!("Failed to write PID file: {e}");
    }

    // Phase 2: wait for health over UDS
    match wait_for_health() {
        Ok(()) => {
            // Give --execute commands time to finish (appservice registration).
            // The health endpoint responds before admin commands complete.
            std::thread::sleep(Duration::from_millis(1500));
            ChatState::get_mut(state).server_status = ServerStatus::Running;
        }
        Err(ref e) => {
            ChatState::get_mut(state).server_status = ServerStatus::Error(e.clone());
            return Err(e.clone());
        }
    }

    Ok(())
}

/// Check if the homeserver is healthy via the versions endpoint over UDS.
///
/// # Errors
///
/// Returns an error if the UDS connection fails or the response is not 2xx.
pub(crate) fn health_check() -> Result<(), String> {
    let _body = uds_get("/_matrix/client/versions")?;
    Ok(())
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

// -- Global directory setup --------------------------------------------------

/// Create the global matrix directory tree with restrictive permissions.
///
/// Creates `~/.context-pilot/matrix/` and its subdirectories with
/// mode 700 (owner-only access).
pub(crate) fn ensure_global_dirs() -> Result<PathBuf, String> {
    let matrix_dir = global_matrix_dir().ok_or("Cannot determine home directory")?;

    for sub in &["", "data", "media", "bridges"] {
        let p = matrix_dir.join(sub);
        std::fs::create_dir_all(&p).map_err(|e| format!("Cannot create {}: {e}", p.display()))?;
    }

    // Restrict top-level dir to owner only
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let perms = std::fs::Permissions::from_mode(0o700);
        let _r = std::fs::set_permissions(&matrix_dir, perms);
    }

    Ok(matrix_dir)
}
