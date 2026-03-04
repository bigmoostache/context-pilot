//! Console Server: persistent daemon that owns child processes.
#![expect(unused_crate_dependencies, reason = "bin target shares Cargo.toml with lib — lib deps aren't used here")]
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
//! cargo build --release -p cp-mod-console
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
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

mod protocol;
use protocol::{Request, Response, SessionInfo, interpret_escapes};

/// Global flag set by signal handler to trigger graceful shutdown.
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Session management
// ---------------------------------------------------------------------------

struct Session {
    pid: u32,
    stdin: Option<std::process::ChildStdin>,
    status: SessionStatus,
}

#[derive(Clone)]
enum SessionStatus {
    Running,
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

    fn status_str(&self) -> String {
        match &self.status {
            SessionStatus::Running => "running".to_string(),
            SessionStatus::Exited(code) => format!("exited({code})"),
        }
    }

    fn exit_code(&self) -> Option<i32> {
        match &self.status {
            SessionStatus::Running => None,
            SessionStatus::Exited(c) => Some(*c),
        }
    }

    fn is_terminal(&self) -> bool {
        matches!(self.status, SessionStatus::Exited(_))
    }
}

fn is_pid_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

type Sessions = Arc<Mutex<HashMap<String, Session>>>;

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

fn handle_create(sessions: &Sessions, key: &str, command: &str, cwd: Option<&str>, log_path: &str) -> Response {
    let log = PathBuf::from(log_path);

    // Create/truncate log file
    if let Some(parent) = log.parent() {
        let _ = std::fs::create_dir_all(parent).ok();
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
    let _ = cmd.args(["-c", command]);
    let _ = cmd.stdin(Stdio::piped()).stdout(log_file).stderr(log_err);

    if let Some(dir) = cwd {
        let _ = cmd.current_dir(dir);
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
            let code = match child.wait() {
                Ok(status) => status.code().unwrap_or(-1),
                Err(_) => -1,
            };
            if let Ok(mut map) = sessions.lock()
                && let Some(session) = map.get_mut(&key)
            {
                session.status = SessionStatus::Exited(code);
            }
        }));
    }

    let session = Session { pid, stdin, status: SessionStatus::Running };
    drop(sessions.lock().unwrap().insert(key.to_string(), session));

    Response::ok_pid(pid)
}

fn handle_send(sessions: &Sessions, key: &str, input: &str) -> Response {
    let bytes = interpret_escapes(input);
    let mut map = sessions.lock().unwrap();
    let Some(session) = map.get_mut(key) else {
        return Response::err(format!("Session '{key}' not found"));
    };
    if session.is_terminal() {
        return Response::err(format!("Session '{key}' already exited"));
    }
    match &mut session.stdin {
        Some(stdin) => {
            if let Err(e) = stdin.write_all(&bytes) {
                return Response::err(format!("Write failed: {e}"));
            }
            if let Err(e) = stdin.flush() {
                return Response::err(format!("Flush failed: {e}"));
            }
            Response::ok()
        }
        None => Response::err("No stdin available".to_string()),
    }
}

fn handle_kill(sessions: &Sessions, key: &str) -> Response {
    let mut map = sessions.lock().unwrap();
    let Some(session) = map.get_mut(key) else {
        return Response::err(format!("Session '{key}' not found"));
    };
    if !session.is_terminal() {
        // SIGTERM to script PID only — PTY teardown propagates SIGHUP to children
        drop(Command::new("kill").args([&session.pid.to_string()]).output());
        std::thread::sleep(std::time::Duration::from_millis(100));
        if is_pid_alive(session.pid) {
            drop(Command::new("kill").args(["-9", &session.pid.to_string()]).output());
        }
        session.status = SessionStatus::Exited(-9);
    }
    // Drop stdin
    drop(session.stdin.take());
    Response::ok()
}

fn handle_remove(sessions: &Sessions, key: &str) -> Response {
    let mut map = sessions.lock().unwrap();
    if let Some(mut session) = map.remove(key) {
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

fn handle_status(sessions: &Sessions, key: &str) -> Response {
    let mut map = sessions.lock().unwrap();
    let Some(session) = map.get_mut(key) else {
        return Response::err(format!("Session '{key}' not found"));
    };
    session.poll_status();
    Response::ok_status(session.status_str(), session.exit_code())
}

fn handle_list(sessions: &Sessions) -> Response {
    let mut map = sessions.lock().unwrap();
    let infos: Vec<SessionInfo> = map
        .iter_mut()
        .map(|(key, session)| {
            session.poll_status();
            SessionInfo {
                key: key.clone(),
                pid: session.pid,
                status: session.status_str(),
                exit_code: session.exit_code(),
            }
        })
        .collect();
    Response::ok_sessions(infos)
}

// ---------------------------------------------------------------------------
// Connection handler
// ---------------------------------------------------------------------------

#[expect(clippy::needless_pass_by_value, reason = "thread entry point — Arc is moved from spawn closure")]
fn handle_connection(stream: UnixStream, sessions: Sessions) {
    let reader = BufReader::new(stream.try_clone().unwrap());
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
                drop(writeln!(writer, "{}", serde_json::to_string(&resp).unwrap()));
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
                    handle_create(&sessions, key, command, req.cwd.as_deref(), log_path)
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
                // Kill all sessions and exit
                let mut map = sessions.lock().unwrap();
                for (_, session) in map.iter_mut() {
                    if !session.is_terminal() {
                        drop(Command::new("kill").args([&session.pid.to_string()]).output());
                    }
                    drop(session.stdin.take());
                }
                map.clear();
                let resp = Response::ok();
                drop(writeln!(writer, "{}", serde_json::to_string(&resp).unwrap()));
                std::process::exit(0);
            }
            other => Response::err(format!("Unknown command: {other}")),
        };

        if writeln!(writer, "{}", serde_json::to_string(&resp).unwrap()).is_err() {
            break; // Connection lost
        }
    }
}

// ---------------------------------------------------------------------------
// Main: daemonize and listen
// ---------------------------------------------------------------------------

fn main() {
    let socket_path = std::env::args().nth(1).expect("Usage: cp-console-server <socket_path>");
    let pid_path = format!("{}.pid", socket_path.trim_end_matches(".sock"));

    // Remove stale socket
    let _ = std::fs::remove_file(&socket_path).ok();

    // Note: setsid() is called by the TUI at spawn time (pre_exec hook).
    // The server is already a session leader when it reaches main().

    // Write PID file
    let _ = std::fs::write(&pid_path, format!("{}", std::process::id())).ok();

    // Bind socket
    let listener = match UnixListener::bind(&socket_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind {socket_path}: {e}");
            std::process::exit(1);
        }
    };

    // Set socket to non-blocking so we can check SHUTDOWN_REQUESTED between accepts
    let _ = listener.set_nonblocking(true).ok();

    let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));

    // Install SIGTERM/SIGINT handlers — set flag, cleanup thread does the rest
    install_signal_handlers();

    // Spawn a shutdown monitor thread that watches SHUTDOWN_REQUESTED
    {
        let sessions = Arc::clone(&sessions);
        let socket_path = socket_path.clone();
        let pid_path = pid_path.clone();
        drop(std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
                    kill_all_sessions(&sessions);
                    let _ = std::fs::remove_file(&socket_path).ok();
                    let _ = std::fs::remove_file(&pid_path).ok();
                    std::process::exit(0);
                }
            }
        }));
    }

    // Accept connections (one thread per connection)
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let sessions = Arc::clone(&sessions);
                drop(std::thread::spawn(move || {
                    handle_connection(stream, sessions);
                }));
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No pending connection — sleep briefly and retry
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => continue,
        }

        if SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
            kill_all_sessions(&sessions);
            let _ = std::fs::remove_file(&socket_path).ok();
            let _ = std::fs::remove_file(&pid_path).ok();
            std::process::exit(0);
        }
    }
}

/// Install SIGTERM and SIGINT handlers that set `SHUTDOWN_REQUESTED`.
#[expect(
    unsafe_code,
    reason = "libc::signal requires unsafe — signal handler is async-signal-safe (atomic store only)"
)]
fn install_signal_handlers() {
    unsafe {
        let _ = libc::signal(libc::SIGTERM, signal_handler as *const () as libc::sighandler_t);
        let _ = libc::signal(libc::SIGINT, signal_handler as *const () as libc::sighandler_t);
        let _ = libc::signal(libc::SIGHUP, signal_handler as *const () as libc::sighandler_t);
    }
}

extern "C" fn signal_handler(_sig: libc::c_int) {
    SHUTDOWN_REQUESTED.store(true, Ordering::Relaxed);
}

/// Kill all sessions — used during shutdown.
fn kill_all_sessions(sessions: &Sessions) {
    let mut map = sessions.lock().unwrap();
    for (_, session) in map.iter_mut() {
        if !session.is_terminal() {
            drop(Command::new("kill").args([&session.pid.to_string()]).output());
            std::thread::sleep(std::time::Duration::from_millis(50));
            if is_pid_alive(session.pid) {
                drop(Command::new("kill").args(["-9", &session.pid.to_string()]).output());
            }
        }
        drop(session.stdin.take());
    }
    map.clear();
}
