//! TUI-side session handles for processes owned by the console server.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cp_base::panels::now_ms;

use crate::pollers::{DetachedLaunch, FilePoller, StatusPoller, create_request, start_file_poller};
use crate::ring_buffer::{Matcher, RingBuffer};
use crate::types::ProcessStatus;
use cp_base::cast::Safe as _;

pub mod client;

use self::client::{find_or_create_server, log_file_path, server_request};

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
    /// Buffer offset where the next `console_wait` pattern search starts:
    /// advanced past each match and on every `send_input`, so a wait never
    /// fires on output an earlier wait or command already consumed.
    pub wait_cursor: Arc<AtomicU64>,
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

/// Ask the server whether a reconnected session is still alive, folding a
/// dead/exited answer straight into the shared status handles.
///
/// Returns `true` if the session is still running (caller should start pollers),
/// `false` if the server doesn't know it or reports it exited — in which case
/// `status`/`finished_at`/`stop_polling` are updated to a terminal state here.
fn probe_session_alive(
    name: &str,
    status: &Arc<Mutex<ProcessStatus>>,
    finished_at: &Arc<Mutex<Option<u64>>>,
    stop_polling: &Arc<AtomicBool>,
) -> bool {
    let mark_terminal = |code: i32| {
        {
            let mut s = status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            *s = ProcessStatus::Finished(code);
        }
        {
            let mut fin = finished_at.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            *fin = Some(now_ms());
        }
        stop_polling.store(true, Ordering::Relaxed);
    };

    let req = serde_json::json!({"cmd": "status", "key": name});
    server_request(&req).map_or_else(
        |_| {
            // Server doesn't know about this session — mark dead.
            mark_terminal(-1i32);
            false
        },
        |resp| {
            let st = resp.get("status").and_then(|v| v.as_str()).unwrap_or("");
            if st.starts_with("exited") {
                let code = resp.get("exit_code").and_then(serde_json::Value::as_i64).unwrap_or(-1).to_i32();
                mark_terminal(code);
                false
            } else {
                true // running
            }
        },
    )
}

impl SessionHandle {
    /// Spawn a child via the console server without blocking the main loop:
    /// returns a `Running` handle at once and sends `create` from a worker
    /// agent (the round-trip took up to 127 ms on the loop). The pid is filled
    /// once the server answers; a spawn error lands in the output buffer and
    /// flips the status to `Failed(-1)`.
    #[must_use]
    pub fn spawn_detached(name: String, command: String, cwd: Option<String>) -> Self {
        let handle = Self::unstarted(name, command, cwd);
        start_file_poller(PathBuf::from(&handle.log_path), handle.buffer.clone(), Arc::clone(&handle.stop_polling));
        let launch = DetachedLaunch {
            req: create_request(&handle.name, &handle.command, &handle.log_path, handle.cwd.as_deref()),
            child_id: Arc::clone(&handle.child_id),
            buffer: handle.buffer.clone(),
            poller: handle.status_poller(),
        };
        drop(std::thread::spawn(move || launch.run()));
        handle
    }

    /// `Running` handle with no pid yet and no poller threads started.
    fn unstarted(name: String, command: String, cwd: Option<String>) -> Self {
        let log_path = log_file_path(&name).to_string_lossy().to_string();
        Self {
            name,
            command,
            cwd,
            status: Arc::new(Mutex::new(ProcessStatus::Running)),
            buffer: RingBuffer::new(),
            log_path,
            child_id: Arc::new(Mutex::new(None)),
            started_at: now_ms(),
            finished_at: Arc::new(Mutex::new(None)),
            stop_polling: Arc::new(AtomicBool::new(false)),
            wait_cursor: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Match `matcher` against output after the wait cursor; on a hit, advance
    /// the cursor past the match so the same output never satisfies a later wait.
    #[must_use]
    pub fn take_pattern_match(&self, matcher: &Matcher) -> bool {
        let Some(end) = self.buffer.find_since(self.wait_cursor.load(Ordering::Relaxed), matcher) else {
            return false;
        };
        let _prev = self.wait_cursor.fetch_max(end, Ordering::Relaxed);
        true
    }

    /// Status poller bound to this handle's shared status/finish/stop state.
    fn status_poller(&self) -> StatusPoller {
        StatusPoller {
            key: self.name.clone(),
            status: Arc::clone(&self.status),
            finished_at: Arc::clone(&self.finished_at),
            stop: Arc::clone(&self.stop_polling),
        }
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
        // Pre-reload output was already visible to earlier waits.
        let wait_cursor = Arc::new(AtomicU64::new(buffer.total_written()));

        // Check if server knows about this session
        let server_alive = probe_session_alive(&name, &status, &finished_at, &stop_polling);

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
        }

        let handle = Self {
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
            wait_cursor,
        };
        if server_alive {
            let poller = handle.status_poller();
            drop(std::thread::spawn(move || poller.run()));
        }
        handle
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
        // Waits after this input should only match the reply to it.
        let _prev = self.wait_cursor.fetch_max(self.buffer.total_written(), Ordering::Relaxed);
        if server_request(&req).is_err() {
            // Server may have died — try to respawn and retry
            let _p = cp_base::perf_span!("console_respawn_server");
            find_or_create_server()?;
            drop(server_request(&req)?);
        }
        Ok(())
    }

    /// Kill the process without blocking the caller.
    ///
    /// The server reaps with a SIGTERM grace period (100+ ms), which froze the
    /// main loop on `Close_panel` and callback dedup. The request runs on a
    /// detached thread; local status flips to `Killed` immediately.
    pub fn kill(&self) {
        self.mark_killed();
        let req = serde_json::json!({"cmd": "kill", "key": self.name});
        let bg_req = req.clone();
        let spawned =
            std::thread::Builder::new().name("console-kill".into()).spawn(move || drop(server_request(&bg_req).ok()));
        if spawned.is_err() {
            drop(server_request(&req).ok());
        }
    }

    /// Kill the process and wait for the server to confirm (shutdown paths).
    pub fn kill_blocking(&self) {
        self.mark_killed();
        let req = serde_json::json!({"cmd": "kill", "key": self.name});
        drop(server_request(&req).ok());
    }

    /// Stop polling and record a terminal `Killed` status + finish time.
    fn mark_killed(&self) {
        self.stop_polling.store(true, Ordering::Relaxed);
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
