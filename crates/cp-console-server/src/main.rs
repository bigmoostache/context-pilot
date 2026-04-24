//! Console Server: persistent daemon that owns child processes.
//!
//! Spawns `sh -c` processes with stdout/stderr redirected to log files.
//! TUI communicates via JSON lines over a Unix socket.
//! Survives TUI exit/reload — processes stay alive.
//!
//! # Process cleanup
//!
//! On SIGTERM/SIGINT, the server kills all children and exits cleanly.
//! On abnormal death (SIGKILL), children become orphans — the TUI's
//! orphan cleanup handles them on next startup.
//!
//! # Rebuilding after changes
//!
//! The server is a standalone binary (`cp-console-server`) built from this file.
//! Since it runs as a long-lived daemon, code changes require a manual restart:
//!
//! ```sh
//! # 1. Build the new binary
//! cargo build --release -p cp-console-server
//!
//! # 2. Kill the running server (TUI auto-restarts it on next launch)
//! kill $(cat .context-pilot/console/server.pid)
//!
//! # 3. Clean stale socket/pid files
//! rm -f .context-pilot/console/server.sock .context-pilot/console/server.pid
//! ```
//!
//! The TUI's `find_or_create_server()` will spawn the new binary automatically
//! on next launch or module reload.

use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

/// JSON protocol types and escape-sequence helpers shared with the TUI client.
mod protocol;
use protocol::{Request, Response, SessionInfo, interpret_escapes};

/// Global flag set by signal handler to trigger graceful shutdown.
/// Wrapped in `Arc` so `signal-hook` can register it directly.
static SHUTDOWN_REQUESTED: LazyLock<Arc<AtomicBool>> = LazyLock::new(|| Arc::new(AtomicBool::new(false)));

// ---------------------------------------------------------------------------
// Session management
// ---------------------------------------------------------------------------

/// State of a single managed child process.
struct Session {
    /// PID of the child process.
    pid: u32,
    /// Handle to the child's stdin pipe, used by `"send"` commands.
    stdin: Option<std::process::ChildStdin>,
    /// Current lifecycle status of the child process.
    status: SessionStatus,
}

/// Lifecycle status of a managed session.
#[derive(Clone)]
enum SessionStatus {
    /// The child process is still running.
    Running,
    /// The child process has exited with the given exit code.
    Exited(i32),
}

impl Session {
    /// Check if the process has exited (non-blocking).
    fn poll_status(&mut self) {
        if matches!(self.status, SessionStatus::Running) && !is_pid_alive(self.pid) {
            // Try to get exit code from /proc/{pid}/status or fall back to -1
            self.status = SessionStatus::Exited(-1);
        }
    }

    /// Return a human-readable status string for use in responses.
    fn status_str(&self) -> String {
        match &self.status {
            SessionStatus::Running => "running".to_string(),
            SessionStatus::Exited(code) => format!("exited({code})"),
        }
    }

    /// Return the exit code if the session has terminated, otherwise `None`.
    const fn exit_code(&self) -> Option<i32> {
        match &self.status {
            SessionStatus::Running => None,
            SessionStatus::Exited(c) => Some(*c),
        }
    }

    /// Return `true` if the session has reached a terminal (exited) state.
    const fn is_terminal(&self) -> bool {
        matches!(self.status, SessionStatus::Exited(_))
    }
}

/// Return `true` if the process with the given PID is still alive (non-blocking signal-0 probe).
fn is_pid_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Shared, thread-safe map from session key to [`Session`].
type Sessions = Arc<Mutex<HashMap<String, Session>>>;

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

/// Parameters for spawning a new child process.
struct CreateParams<'req> {
    /// Session key that identifies the target child process.
    key: &'req str,
    /// Shell command to execute.
    command: &'req str,
    /// Optional working directory for the spawned process.
    cwd: Option<&'req str>,
    /// Path of the log file for stdout/stderr redirection.
    log_path: &'req str,
}

/// Spawn a new child process and register it under `key`, redirecting output to `log_path`.
fn handle_create(sessions: &Sessions, params: &CreateParams<'_>) -> Response {
    let CreateParams { key, command, cwd, log_path } = params;
    let log = PathBuf::from(log_path);

    // Create/truncate log file
    if let Some(parent) = log.parent() {
        let _: Option<()> = std::fs::create_dir_all(parent).ok();
    }

    let log_file = match std::fs::File::create(&log) {
        Ok(f) => f,
        Err(e) => return Response::err(format!("Failed to create log: {e}")),
    };
    let log_err = match log_file.try_clone() {
        Ok(f) => f,
        Err(e) => return Response::err(format!("Failed to clone log fd: {e}")),
    };

    let mut cmd = Command::new("sh");
    let _: &mut Command = cmd.args(["-c", command]);
    let _: &mut Command = cmd.stdin(Stdio::piped()).stdout(log_file).stderr(log_err);

    if let Some(dir) = cwd {
        let _: &mut Command = cmd.current_dir(dir);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Response::err(format!("Spawn failed: {e}")),
    };

    let pid = child.id();
    let stdin = child.stdin.take();

    // Spawn a thread to wait for the child so we get proper exit status
    {
        let sessions = Arc::clone(sessions);
        let key = key.to_string();
        drop(std::thread::spawn(move || {
            let code = child.wait().map_or(-1, |status| status.code().unwrap_or(-1));
            if let Ok(mut map) = sessions.lock()
                && let Some(session) = map.get_mut(&key)
            {
                session.status = SessionStatus::Exited(code);
            }
        }));
    }

    let session = Session { pid, stdin, status: SessionStatus::Running };
    drop(sessions.lock().unwrap_or_else(PoisonError::into_inner).insert(key.to_string(), session));

    Response::ok_pid(pid)
}

/// Write `input` (with escape sequences interpreted) to the stdin of session `key`.
fn handle_send(sessions: &Sessions, key: &str, input: &str) -> Response {
    let bytes = interpret_escapes(input);

    // Take stdin out of session (lock held briefly — released before I/O)
    let mut stdin = match sessions.lock().unwrap_or_else(PoisonError::into_inner).get_mut(key) {
        None => return Response::err(format!("Session '{key}' not found")),
        Some(session) if session.is_terminal() => {
            return Response::err(format!("Session '{key}' already exited"));
        }
        Some(session) => match session.stdin.take() {
            Some(s) => s,
            None => return Response::err("No stdin available".to_string()),
        },
    };

    // Perform I/O without holding the session lock
    let result = stdin.write_all(&bytes).and_then(|()| stdin.flush());

    // Put stdin back
    if let Some(session) = sessions.lock().unwrap_or_else(PoisonError::into_inner).get_mut(key) {
        session.stdin = Some(stdin);
    }

    result.map_or_else(|e| Response::err(format!("Write failed: {e}")), |()| Response::ok())
}

/// Terminate the child process for session `key` (SIGTERM then SIGKILL if needed).
fn handle_kill(sessions: &Sessions, key: &str) -> Response {
    // Extract pid under lock (lock auto-drops after match expression)
    let pid = match sessions.lock().unwrap_or_else(PoisonError::into_inner).get_mut(key) {
        None => return Response::err(format!("Session '{key}' not found")),
        Some(session) if session.is_terminal() => {
            drop(session.stdin.take());
            return Response::ok();
        }
        Some(session) => session.pid,
    };

    // Kill process without holding the session lock
    drop(Command::new("kill").args([&pid.to_string()]).output());
    std::thread::sleep(std::time::Duration::from_millis(100));
    if is_pid_alive(pid) {
        drop(Command::new("kill").args(["-9", &pid.to_string()]).output());
    }

    // Update session state under lock
    if let Some(session) = sessions.lock().unwrap_or_else(PoisonError::into_inner).get_mut(key) {
        session.status = SessionStatus::Exited(-9);
        drop(session.stdin.take());
    }
    Response::ok()
}

/// Kill (if still running) and remove the session for `key` from the session map.
fn handle_remove(sessions: &Sessions, key: &str) -> Response {
    let removed = sessions.lock().unwrap_or_else(PoisonError::into_inner).remove(key);
    if let Some(mut session) = removed {
        if !session.is_terminal() {
            drop(Command::new("kill").args([&session.pid.to_string()]).output());
            std::thread::sleep(std::time::Duration::from_millis(100));
            if is_pid_alive(session.pid) {
                drop(Command::new("kill").args(["-9", &session.pid.to_string()]).output());
            }
        }
        drop(session.stdin.take());
    }
    Response::ok()
}

/// Query the current status and exit code of session `key`.
fn handle_status(sessions: &Sessions, key: &str) -> Response {
    let mut map = sessions.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(session) = map.get_mut(key) else {
        return Response::err(format!("Session '{key}' not found"));
    };
    session.poll_status();
    let status = session.status_str();
    let exit_code = session.exit_code();
    drop(map);
    Response::ok_status(status, exit_code)
}

/// Return a snapshot of all currently registered sessions and their statuses.
fn handle_list(sessions: &Sessions) -> Response {
    let infos: Vec<SessionInfo> = {
        let mut map = sessions.lock().unwrap_or_else(PoisonError::into_inner);
        map.iter_mut()
            .map(|(key, session)| {
                session.poll_status();
                SessionInfo {
                    key: key.clone(),
                    pid: session.pid,
                    status: session.status_str(),
                    exit_code: session.exit_code(),
                }
            })
            .collect()
    };
    Response::ok_sessions(infos)
}

// ---------------------------------------------------------------------------
// Connection handler
// ---------------------------------------------------------------------------

/// Per-connection handler. Owns the stream and sessions Arc for `thread::spawn`.
struct ConnectionHandler {
    /// Unix socket stream for this client connection.
    stream: UnixStream,
    /// Shared session map passed down from the accept loop.
    sessions: Sessions,
}

impl ConnectionHandler {
    /// Consume self and process JSON-line commands until the connection closes.
    fn run(self) {
        let Self { stream, sessions } = self;
        let Ok(cloned) = stream.try_clone() else {
            return;
        };
        let reader = BufReader::new(cloned);
        let mut writer = stream;

        for line in reader.lines() {
            let Ok(line) = line else {
                break; // Connection closed
            };
            if line.is_empty() {
                continue;
            }

            let req: Request = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(e) => {
                    let resp = Response::err(format!("Invalid JSON: {e}"));
                    drop(writeln!(writer, "{}", serde_json::to_string(&resp).unwrap_or_default()));
                    continue;
                }
            };

            let resp = match req.cmd.as_str() {
                "create" => {
                    let key = req.key.as_deref().unwrap_or("");
                    let command = req.command.as_deref().unwrap_or("");
                    let log_path = req.log_path.as_deref().unwrap_or("");
                    if key.is_empty() || command.is_empty() || log_path.is_empty() {
                        Response::err("Missing key, command, or log_path")
                    } else {
                        handle_create(&sessions, &CreateParams { key, command, cwd: req.cwd.as_deref(), log_path })
                    }
                }
                "send" => {
                    let key = req.key.as_deref().unwrap_or("");
                    let input = req.input.as_deref().unwrap_or("");
                    if key.is_empty() { Response::err("Missing key") } else { handle_send(&sessions, key, input) }
                }
                "kill" => {
                    let key = req.key.as_deref().unwrap_or("");
                    if key.is_empty() { Response::err("Missing key") } else { handle_kill(&sessions, key) }
                }
                "remove" => {
                    let key = req.key.as_deref().unwrap_or("");
                    if key.is_empty() { Response::err("Missing key") } else { handle_remove(&sessions, key) }
                }
                "status" => {
                    let key = req.key.as_deref().unwrap_or("");
                    if key.is_empty() { Response::err("Missing key") } else { handle_status(&sessions, key) }
                }
                "list" => handle_list(&sessions),
                "ping" => Response::ok(),
                "shutdown" => {
                    SHUTDOWN_REQUESTED.store(true, Ordering::Relaxed);
                    let resp = Response::ok();
                    drop(writeln!(writer, "{}", serde_json::to_string(&resp).unwrap_or_default()));
                    break;
                }
                other => Response::err(format!("Unknown command: {other}")),
            };

            if writeln!(writer, "{}", serde_json::to_string(&resp).unwrap_or_default()).is_err() {
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Main: daemonize and listen
// ---------------------------------------------------------------------------

/// Entry point: parse arguments, bind the Unix socket, and start the accept loop.
fn main() {
    let Some(socket_path) = std::env::args().nth(1) else {
        drop(writeln!(std::io::stderr(), "Usage: cp-console-server <socket_path>"));
        return;
    };
    let pid_path = format!("{}.pid", socket_path.trim_end_matches(".sock"));

    // Remove stale socket
    let _: Option<()> = std::fs::remove_file(&socket_path).ok();

    // Become a session leader so children get SIGHUP when the server dies.
    #[cfg(unix)]
    {
        let _r = nix::unistd::setsid();
    }

    // Write PID file
    let _: Option<()> = std::fs::write(&pid_path, format!("{}", std::process::id())).ok();

    // Bind socket
    let Ok(listener) = UnixListener::bind(&socket_path) else {
        drop(writeln!(std::io::stderr(), "Failed to bind {socket_path}"));
        return;
    };

    // Set socket to non-blocking so we can check SHUTDOWN_REQUESTED between accepts
    let _: Option<()> = listener.set_nonblocking(true).ok();

    let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));

    // Install SIGTERM/SIGINT handlers — set flag, main loop polls it
    install_signal_handlers();

    // Accept connections (one thread per connection)
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let sessions = Arc::clone(&sessions);
                drop(std::thread::spawn(move || {
                    ConnectionHandler { stream, sessions }.run();
                }));
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No pending connection — sleep briefly and retry
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => continue,
        }

        if SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
            break;
        }
    }

    // Cleanup: kill children, remove socket/pid files
    kill_all_sessions(&sessions);
    let _: Option<()> = std::fs::remove_file(&socket_path).ok();
    let _: Option<()> = std::fs::remove_file(&pid_path).ok();
}

/// Register SIGINT and SIGHUP handlers via `signal-hook`.
///
/// Each handler atomically sets [`SHUTDOWN_REQUESTED`] — the main accept loop
/// polls it and breaks cleanly.
fn install_signal_handlers() {
    for sig in [signal_hook::consts::SIGINT, signal_hook::consts::SIGHUP] {
        drop(signal_hook::flag::register(sig, Arc::clone(&SHUTDOWN_REQUESTED)));
    }
}

// Here be the last port of call — once ye enter, no process leaves alive.
/// Kill all sessions — used during shutdown.
fn kill_all_sessions(sessions: &Sessions) {
    let mut map = sessions.lock().unwrap_or_else(PoisonError::into_inner);
    let mut keys: Vec<_> = map.keys().cloned().collect();
    keys.sort();
    for key in &keys {
        if let Some(session) = map.get_mut(key) {
            if !session.is_terminal() {
                drop(Command::new("kill").args([&session.pid.to_string()]).output());
                std::thread::sleep(std::time::Duration::from_millis(50));
                if is_pid_alive(session.pid) {
                    drop(Command::new("kill").args(["-9", &session.pid.to_string()]).output());
                }
            }
            drop(session.stdin.take());
        }
    }
    map.clear();
}
