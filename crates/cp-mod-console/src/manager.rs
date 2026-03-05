use std::collections::HashSet;
use std::fs;
use std::hash::BuildHasher;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cp_base::config::constants;
use cp_base::panels::now_ms;

use crate::CONSOLE_DIR;
use crate::pollers::{FilePoller, StatusPoller};
use crate::ring_buffer::RingBuffer;
use crate::types::ProcessStatus;
use cp_base::cast::Safe as _;

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
    if base.is_absolute() { base } else { std::env::current_dir().unwrap_or_default().join(base) }
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
    let stream = UnixStream::connect(&sock_path).map_err(|e| format!("Failed to connect to console server: {e}"))?;
    let _: Option<()> = stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok();
    let _: Option<()> = stream.set_write_timeout(Some(std::time::Duration::from_secs(5))).ok();

    let mut writer = stream.try_clone().map_err(|e| format!("Clone failed: {e}"))?;
    let reader = BufReader::new(stream);

    let mut line = serde_json::to_string(req).map_err(|e| format!("Serialize failed: {e}"))?;
    line.push('\n');
    writer.write_all(line.as_bytes()).map_err(|e| format!("Write failed: {e}"))?;
    writer.flush().map_err(|e| format!("Flush failed: {e}"))?;

    let mut resp_line = String::new();
    let mut buf_reader = reader;
    let _: usize = buf_reader.read_line(&mut resp_line).map_err(|e| format!("Read failed: {e}"))?;

    let resp: serde_json::Value =
        serde_json::from_str(resp_line.trim()).map_err(|e| format!("Parse response failed: {e}"))?;

    if resp.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        Ok(resp)
    } else {
        let err = resp.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error");
        Err(err.to_string())
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

    // Wait for socket to appear (up to 3 seconds)
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if server_request(&ping).is_ok() {
            return Ok(());
        }
    }

    Err("Console server failed to start within 3 seconds".to_string())
}

/// Kill orphaned processes by asking the server for its session list and
/// comparing against known session keys.
pub fn kill_orphaned_processes<S: BuildHasher>(known_keys: &HashSet<String, S>) {
    let list = serde_json::json!({"cmd": "list"});
    if let Ok(resp) = server_request(&list)
        && let Some(sessions) = resp.get("sessions").and_then(|v| v.as_array())
    {
        for session in sessions {
            if let Some(key) = session.get("key").and_then(|v| v.as_str())
                && !known_keys.contains(key)
            {
                // Orphan — remove it from server (kills process)
                let remove = serde_json::json!({"cmd": "remove", "key": key});
                drop(server_request(&remove).ok());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// SessionHandle — TUI-side handle for a server-managed process
// ---------------------------------------------------------------------------

/// A managed child process session.
/// The process is owned by the console server.
/// The TUI polls the log file for output into a `RingBuffer`.
#[derive(Debug)]
pub struct SessionHandle {
    /// Unique session key (e.g., "`c_42`").
    pub name: String,
    /// Shell command that was executed.
    pub command: String,
    /// Working directory (None = project root).
    pub cwd: Option<String>,
    /// Current process status (shared with status poller thread).
    pub status: Arc<Mutex<ProcessStatus>>,
    /// Ring buffer capturing stdout/stderr output.
    pub buffer: RingBuffer,
    /// Absolute path to the log file.
    pub log_path: String,
    /// Server-reported PID (shared with poller).
    pub child_id: Arc<Mutex<Option<u32>>>,
    /// Timestamp (ms since epoch) when spawned.
    pub started_at: u64,
    /// Timestamp when process exited (shared with status poller).
    pub finished_at: Arc<Mutex<Option<u64>>>,
    /// Signal to stop background poller threads.
    pub stop_polling: Arc<AtomicBool>,
}

/// Parameters for reconnecting to an existing server-managed session.
#[derive(Debug)]
pub struct ReconnectMeta {
    /// Unique session key.
    pub name: String,
    /// Shell command that was executed.
    pub command: String,
    /// Working directory (None = project root).
    pub cwd: Option<String>,
    /// Server-reported PID.
    pub pid: u32,
    /// Absolute path to the log file.
    pub log_path_str: String,
    /// Timestamp (ms since epoch) when originally spawned.
    pub started_at: u64,
}

impl SessionHandle {
    /// Spawn a new child process via the console server.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the server is unreachable and cannot be restarted,
    /// or if the spawn request fails.
    pub fn spawn(name: String, command: String, cwd: Option<String>) -> Result<Self, String> {
        let log_path = log_file_path(&name);
        let log_path_str = log_path.to_string_lossy().to_string();

        // Ask server to create the process
        let mut req = serde_json::json!({
            "cmd": "create",
            "key": name,
            "command": command,
            "log_path": log_path_str,
        });
        if let Some(ref dir) = cwd
            && let Some(obj) = req.as_object_mut()
        {
            let _prev = obj.insert("cwd".to_string(), serde_json::Value::String(dir.clone()));
        }

        let resp = if let Ok(r) = server_request(&req) {
            r
        } else {
            // Server may have died — try to respawn
            find_or_create_server()?;
            server_request(&req)?
        };
        let pid = resp.get("pid").and_then(serde_json::Value::as_u64).unwrap_or(0).to_u32();

        let status = Arc::new(Mutex::new(ProcessStatus::Running));
        let buffer = RingBuffer::new();
        let child_id = Arc::new(Mutex::new(Some(pid)));
        let finished_at = Arc::new(Mutex::new(None));
        let stop_polling = Arc::new(AtomicBool::new(false));

        // File poller thread
        {
            let buf = buffer.clone();
            let stop = Arc::clone(&stop_polling);
            let path = log_path;
            drop(std::thread::spawn(move || {
                FilePoller { path, buffer: buf, stop, offset: 0 }.run();
            }));
        }

        // Status poller thread — periodically ask server for status
        {
            let status_clone = Arc::clone(&status);
            let finished_clone = Arc::clone(&finished_at);
            let stop_clone = Arc::clone(&stop_polling);
            let key = name.clone();
            drop(std::thread::spawn(move || {
                StatusPoller { key, status: status_clone, finished_at: finished_clone, stop: stop_clone }.run();
            }));
        }

        Ok(Self {
            name,
            command,
            cwd,
            status,
            buffer,
            log_path: log_path_str,
            child_id,
            started_at: now_ms(),
            finished_at,
            stop_polling,
        })
    }

    /// Reconnect to a server-managed session after TUI reload.
    #[must_use]
    pub fn reconnect(meta: ReconnectMeta) -> Self {
        let ReconnectMeta { name, command, cwd, pid, log_path_str, started_at } = meta;
        let log_path = PathBuf::from(&log_path_str);
        let status = Arc::new(Mutex::new(ProcessStatus::Running));
        let buffer = RingBuffer::new();
        let child_id = Arc::new(Mutex::new(Some(pid)));
        let finished_at = Arc::new(Mutex::new(None));
        let stop_polling = Arc::new(AtomicBool::new(false));

        // Load existing log file contents into ring buffer
        let file_offset = fs::read(&log_path).map_or(0, |content| {
            if !content.is_empty() {
                buffer.write(&content);
            }
            content.len().to_u64()
        });

        // Check if server knows about this session
        let server_alive = {
            let req = serde_json::json!({"cmd": "status", "key": name});
            server_request(&req).map_or_else(
                |_| {
                    // Server doesn't know about this session — mark dead
                    {
                        let mut s = status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        *s = ProcessStatus::Finished(-1);
                    }
                    {
                        let mut fin = finished_at.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        *fin = Some(now_ms());
                    }
                    stop_polling.store(true, Ordering::Relaxed);
                    false
                },
                |resp| {
                    let st = resp.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    if st.starts_with("exited") {
                        let code = resp.get("exit_code").and_then(serde_json::Value::as_i64).unwrap_or(-1).to_i32();
                        {
                            let mut s = status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                            *s = ProcessStatus::Finished(code);
                        }
                        {
                            let mut fin = finished_at.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                            *fin = Some(now_ms());
                        }
                        stop_polling.store(true, Ordering::Relaxed);
                        false
                    } else {
                        true // running
                    }
                },
            )
        };

        if server_alive {
            // File poller from offset
            {
                let buf = buffer.clone();
                let stop = Arc::clone(&stop_polling);
                let path = log_path;
                drop(std::thread::spawn(move || {
                    FilePoller { path, buffer: buf, stop, offset: file_offset }.run();
                }));
            }

            // Status poller
            {
                let status_clone = Arc::clone(&status);
                let finished_clone = Arc::clone(&finished_at);
                let stop_clone = Arc::clone(&stop_polling);
                let key = name.clone();
                drop(std::thread::spawn(move || {
                    StatusPoller { key, status: status_clone, finished_at: finished_clone, stop: stop_clone }.run();
                }));
            }
        }

        Self {
            name,
            command,
            cwd,
            status,
            buffer,
            log_path: log_path_str,
            child_id,
            started_at,
            finished_at,
            stop_polling,
        }
    }

    /// Send input to the process via the server.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the server is unreachable and cannot be restarted.
    pub fn send_input(&self, input: &str) -> Result<(), String> {
        let req = serde_json::json!({
            "cmd": "send",
            "key": self.name,
            "input": input,
        });
        if server_request(&req).is_err() {
            // Server may have died — try to respawn and retry
            find_or_create_server()?;
            drop(server_request(&req)?);
        }
        Ok(())
    }

    /// Kill the process via the server.
    pub fn kill(&self) {
        self.stop_polling.store(true, Ordering::Relaxed);

        let req = serde_json::json!({"cmd": "kill", "key": self.name});
        drop(server_request(&req).ok());
        {
            let mut status = self.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if !status.is_terminal() {
                *status = ProcessStatus::Killed;
            }
        }
        let mut fin = self.finished_at.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if fin.is_none() {
            *fin = Some(now_ms());
        }
    }

    /// Get the current process status.
    #[must_use]
    pub fn get_status(&self) -> ProcessStatus {
        *self.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Get exit code (if process is terminal).
    #[must_use]
    pub fn exit_code(&self) -> Option<i32> {
        self.get_status().exit_code()
    }

    /// Get the PID if available.
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        *self.child_id.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// No-op for backward compat — server holds the stdin, not us.
    pub const fn leak_stdin() {}
}
