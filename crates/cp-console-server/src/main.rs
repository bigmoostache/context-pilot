//! Console Server: persistent daemon that owns child processes.
//!
//! Spawns `sh -c` processes with stdout/stderr redirected to log files.
//! TUI communicates via JSON lines over a Unix socket.
//! Survives TUI exit/reload — processes stay alive.
//!
//! # Process cleanup
//!
//! Every child runs in its own process group, so killing a session reaches
//! its whole tree (pipelines, `&&` chains, `npm run dev` grandchildren).
//! On SIGTERM/SIGINT/SIGHUP the server kills every session group, removes its
//! socket and pid files, and exits. On abnormal death (SIGKILL), children
//! become orphans — the TUI's orphan cleanup handles them on next startup.
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
//! # 2. Stop the running server (SIGTERM: kills its children, removes
//! #    server.sock and server.pid). The TUI respawns it on next launch.
//! kill $(cat .context-pilot/console/server.pid)
//! ```
//!
//! The TUI's `find_or_create_server()` will spawn the new binary automatically
//! on next launch or module reload.

use std::collections::BTreeMap;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

/// Process cleanup: reaper thread, graceful shutdown, signal handlers, FD limits.
mod cleanup;
/// JSON protocol types and escape-sequence helpers shared with the TUI client.
mod protocol;
/// Child sessions: process-group spawn, waiter-owned exit status, tree kill.
mod session;
use protocol::{Request, Response, SessionInfo, interpret_escapes};
pub(crate) use session::Session;

/// Global flag set by signal handler to trigger graceful shutdown.
/// Wrapped in `Arc` so `signal-hook` can register it directly.
static SHUTDOWN_REQUESTED: LazyLock<Arc<AtomicBool>> = LazyLock::new(|| Arc::new(AtomicBool::new(false)));

/// Shared, thread-safe map from session key to [`Session`].
type Sessions = Arc<Mutex<BTreeMap<String, Session>>>;

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
///
/// Refuses a key whose session is still running: silently replacing it would
/// orphan the old process (no longer reachable by `kill`).
fn handle_create(sessions: &Sessions, params: &CreateParams<'_>) -> Response {
    let &CreateParams { key, command, cwd, log_path } = params;
    if sessions.lock().unwrap_or_else(PoisonError::into_inner).get(key).is_some_and(|s| !s.is_terminal()) {
        return Response::err(format!("Session '{key}' is already running"));
    }

    let log = PathBuf::from(log_path);
    if let Some(parent) = log.parent() {
        let _: Option<()> = std::fs::create_dir_all(parent).ok();
    }
    let log_file = match std::fs::File::create(&log) {
        Ok(f) => f,
        Err(e) => return Response::err(format!("Failed to create log: {e}")),
    };

    match session::spawn(command, cwd, &log_file) {
        Ok(new_session) => {
            let pid = new_session.pid;
            let mut map = sessions.lock().unwrap_or_else(PoisonError::into_inner);
            drop(map.insert(key.to_owned(), new_session));
            drop(map);
            Response::ok_pid(pid)
        }
        Err(e) => Response::err(e),
    }
}

/// Write `input` (with escape sequences interpreted) to the stdin of session `key`.
fn handle_send(sessions: &Sessions, key: &str, input: &str) -> Response {
    let bytes = interpret_escapes(input);

    // Clone the stdin handle under the lock, write outside it.
    let stdin = match sessions.lock().unwrap_or_else(PoisonError::into_inner).get(key) {
        None => return Response::err(format!("Session '{key}' not found")),
        Some(s) if s.is_terminal() => return Response::err(format!("Session '{key}' already exited")),
        Some(s) => match s.stdin.as_ref() {
            Some(pipe) => Arc::clone(pipe),
            None => return Response::err("No stdin available".to_owned()),
        },
    };

    session::write_with_timeout(stdin, bytes).map_or_else(Response::err, |()| Response::ok())
}

/// Terminate the process tree of session `key` (SIGTERM, then SIGKILL) and
/// remove the session, freeing its stdin pipe FD.
///
/// The group is signalled even when the `sh` leader already exited: background
/// grandchildren it left behind still belong to the group.
fn handle_kill(sessions: &Sessions, key: &str) -> Response {
    let removed = sessions.lock().unwrap_or_else(PoisonError::into_inner).remove(key);
    let Some(mut killed) = removed else {
        return Response::err(format!("Session '{key}' not found"));
    };
    drop(killed.stdin.take());
    session::terminate(killed.pid, 100);
    Response::ok()
}

/// Like [`handle_kill`], but succeeds when the key is unknown.
fn handle_remove(sessions: &Sessions, key: &str) -> Response {
    let removed = sessions.lock().unwrap_or_else(PoisonError::into_inner).remove(key);
    if let Some(mut gone) = removed {
        drop(gone.stdin.take());
        session::terminate(gone.pid, 100);
    }
    Response::ok()
}

/// Query the current status and exit code of session `key`.
fn handle_status(sessions: &Sessions, key: &str) -> Response {
    let map = sessions.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(found) = map.get(key) else {
        return Response::err(format!("Session '{key}' not found"));
    };
    let status = found.status_str();
    let exit_code = found.exit_code();
    drop(map);
    Response::ok_status(status, exit_code)
}

/// Return a snapshot of all currently registered sessions and their statuses.
fn handle_list(sessions: &Sessions) -> Response {
    let infos: Vec<SessionInfo> = sessions
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .map(|(key, s)| SessionInfo { key: key.clone(), pid: s.pid, status: s.status_str(), exit_code: s.exit_code() })
        .collect();
    Response::ok_sessions(infos)
}

/// Return server diagnostic stats: session counts, FD usage, thread count.
fn handle_stats(sessions: &Sessions) -> Response {
    let map = sessions.lock().unwrap_or_else(PoisonError::into_inner);
    let total = map.len();
    let running = map.values().filter(|s| !s.is_terminal()).count();
    let exited = total.saturating_sub(running);
    let stdin_fds = map.values().filter(|s| s.stdin.is_some()).count();
    drop(map);

    // Count open FDs via /dev/fd (macOS/Linux)
    let fd_count = std::fs::read_dir("/dev/fd").map_or(0, Iterator::count);

    // Get rlimit
    let (soft, hard) = rlimit::getrlimit(rlimit::Resource::NOFILE).unwrap_or((0, 0));

    let stats = format!(
        "sessions: {total} (running={running}, exited={exited}), \
         stdin_pipes={stdin_fds}, open_fds={fd_count}, \
         rlimit_soft={soft}, rlimit_hard={hard}, \
         pid={}",
        std::process::id()
    );
    Response::ok_status(stats, None)
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

/// Route a parsed request to its handler, returning the response to send back.
///
/// Covers every command except `"shutdown"` (which needs to break the connection
/// loop and is handled inline by [`ConnectionHandler::run`]).
fn dispatch_command(req: &Request, sessions: &Sessions) -> Response {
    let key = req.key.as_deref().unwrap_or("");
    match req.cmd.as_str() {
        "create" => {
            let command = req.command.as_deref().unwrap_or("");
            let log_path = req.log_path.as_deref().unwrap_or("");
            if key.is_empty() || command.is_empty() || log_path.is_empty() {
                Response::err("Missing key, command, or log_path")
            } else {
                handle_create(sessions, &CreateParams { key, command, cwd: req.cwd.as_deref(), log_path })
            }
        }
        "list" => handle_list(sessions),
        "stats" => handle_stats(sessions),
        "ping" => Response::ok(),
        _ if key.is_empty() => Response::err("Missing key"),
        "send" => handle_send(sessions, key, req.input.as_deref().unwrap_or("")),
        "kill" => handle_kill(sessions, key),
        "remove" => handle_remove(sessions, key),
        "status" => handle_status(sessions, key),
        other => Response::err(format!("Unknown command: {other}")),
    }
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

        for line_result in reader.lines() {
            let Ok(line) = line_result else {
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

            // `shutdown` acks then breaks the loop; every other command routes
            // through dispatch_command.
            if req.cmd == "shutdown" {
                SHUTDOWN_REQUESTED.store(true, Ordering::Relaxed);
                let resp = Response::ok();
                drop(writeln!(writer, "{}", serde_json::to_string(&resp).unwrap_or_default()));
                break;
            }

            let resp = dispatch_command(&req, &sessions);
            if writeln!(writer, "{}", serde_json::to_string(&resp).unwrap_or_default()).is_err() {
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Main: daemonize and listen
// ---------------------------------------------------------------------------

/// Session logs older than this (by mtime) are deleted at server boot.
const LOG_MAX_AGE: std::time::Duration = std::time::Duration::from_hours(168);

/// Entry point: parse arguments, bind the Unix socket, and start the accept loop.
fn main() {
    cleanup::raise_fd_limit();
    let Some(socket_path) = std::env::args().nth(1) else {
        drop(writeln!(std::io::stderr(), "Usage: cp-console-server <socket_path>"));
        return;
    };
    let pid_path = format!("{}.pid", socket_path.trim_end_matches(".sock"));

    // A socket that still accepts belongs to a live server: unlinking it would
    // strand that server (and its children) unreachable. Only remove it stale.
    if UnixStream::connect(&socket_path).is_ok() {
        drop(writeln!(std::io::stderr(), "A console server is already listening on {socket_path}"));
        return;
    }
    let _: Option<()> = std::fs::remove_file(&socket_path).ok();

    // Detach from the launching TUI's session and terminal, so its exit or
    // Ctrl+C does not reach us. Children do NOT die with the server: without
    // a controlling terminal no SIGHUP is ever sent to them.
    let _r = nix::unistd::setsid();

    // Write PID file
    let _: Option<()> = std::fs::write(&pid_path, format!("{}", std::process::id())).ok();

    // Bind socket
    let Ok(listener) = UnixListener::bind(&socket_path) else {
        drop(writeln!(std::io::stderr(), "Failed to bind {socket_path}"));
        return;
    };

    // Blocking accept: a non-blocking poll with a 50 ms idle sleep added 0–50 ms
    // (measured mean ~22 ms) to EVERY client request. Shutdown wakes the accept
    // via a self-connect from `shutdown_waker`.
    let waker_path = socket_path.clone();
    drop(std::thread::spawn(move || cleanup::shutdown_waker(&waker_path)));

    let sessions: Sessions = Arc::new(Mutex::new(BTreeMap::new()));

    // Drop session logs untouched for a week (35k files had piled up, making
    // every directory operation in the console dir slow). Off the accept path.
    {
        let log_dir = std::path::Path::new(&socket_path).parent().map(std::path::Path::to_path_buf);
        drop(std::thread::spawn(move || {
            if let Some(dir) = log_dir {
                cleanup::prune_old_logs(&dir, LOG_MAX_AGE);
            }
        }));
    }

    // SIGTERM/SIGINT/SIGHUP set the shutdown flag; the waker unblocks accept.
    cleanup::install_signal_handlers();

    // Spawn a background thread to auto-reap exited sessions.
    // Without this, sessions that exit but are never explicitly killed accumulate
    // forever — each holding a stdin pipe FD — until FD exhaustion.
    {
        let reaper_sessions = Arc::clone(&sessions);
        drop(std::thread::spawn(move || cleanup::reaper_loop(&reaper_sessions)));
    }

    // Accept connections (one thread per connection)
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
                    break; // the waker's self-connect, or a client racing shutdown
                }
                let conn_sessions = Arc::clone(&sessions);
                drop(std::thread::spawn(move || {
                    ConnectionHandler { stream, sessions: conn_sessions }.run();
                }));
            }
            Err(_) => {
                if SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
    }

    // Cleanup: kill children, remove socket/pid files
    cleanup::kill_all_sessions(&sessions);
    let _: Option<()> = std::fs::remove_file(&socket_path).ok();
    let _: Option<()> = std::fs::remove_file(&pid_path).ok();
}
