//! Process cleanup: reaper thread, graceful shutdown, signal handlers, FD limits.
//!
//! Extracted from `main.rs` to keep it under the 500-line limit.

use std::collections::BTreeMap;
use std::process::Command;
use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};

use crate::{SHUTDOWN_REQUESTED, Session, Sessions, is_pid_alive};

/// Delete `*.log` files in `dir` whose mtime is older than `max_age`.
/// Best-effort: unreadable entries are skipped. A live session keeps writing
/// to its log, so its mtime stays fresh and it is never pruned.
pub(crate) fn prune_old_logs(dir: &std::path::Path, max_age: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("log") {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > max_age);
        if stale {
            drop(std::fs::remove_file(&path));
        }
    }
}

/// Raise the process file-descriptor soft limit. The console server holds
/// pipes, sockets, and log files for every managed child — the macOS default
/// of 256 FDs is easily exhausted. We raise to `min(hard_limit, 8192)`.
pub(crate) fn raise_fd_limit() {
    let Ok((soft, hard)) = rlimit::getrlimit(rlimit::Resource::NOFILE) else {
        return;
    };
    let target = hard.min(8192);
    if soft < target {
        let _r = rlimit::setrlimit(rlimit::Resource::NOFILE, target, hard);
    }
}

/// Register SIGINT and SIGHUP handlers via `signal-hook`.
///
/// Each handler atomically sets [`SHUTDOWN_REQUESTED`] — the main accept loop
/// polls it and breaks cleanly.
pub(crate) fn install_signal_handlers() {
    for sig in [signal_hook::consts::SIGINT, signal_hook::consts::SIGHUP] {
        drop(signal_hook::flag::register(sig, Arc::clone(&SHUTDOWN_REQUESTED)));
    }
}

/// Wait for a shutdown signal, then connect to our own socket once so the
/// main thread's blocking `accept` returns and sees the flag.
pub(crate) fn shutdown_waker(socket_path: &str) {
    while !SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    drop(std::os::unix::net::UnixStream::connect(socket_path));
}

/// Grace period (seconds) after a session exits before the reaper removes it.
/// Gives the TUI time to read the final status and log output.
const REAPER_GRACE_SECS: u64 = 30;

/// Background thread that periodically removes exited sessions from the map.
///
/// Without this, sessions that complete but are never explicitly killed by the
/// TUI accumulate indefinitely — each holding a stdin pipe FD. Over hundreds
/// of callback invocations this exhausts the process file-descriptor limit.
pub(crate) fn reaper_loop(sessions: &Sessions) {
    // Map from session key → first time we observed it as exited (seconds since epoch).
    let mut exit_times: BTreeMap<String, u64> = BTreeMap::new();

    loop {
        if SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
            break;
        }

        std::thread::sleep(std::time::Duration::from_secs(5));

        let now_secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());

        let mut map = sessions.lock().unwrap_or_else(PoisonError::into_inner);
        reap_tick(&mut map, &mut exit_times, now_secs);
        drop(map);
    }
}

/// One reaper pass over the locked session map: record newly-exited sessions,
/// remove those exited past the grace period (freeing their pipe FDs), and drop
/// bookkeeping for sessions removed elsewhere.
fn reap_tick(map: &mut BTreeMap<String, Session>, exit_times: &mut BTreeMap<String, u64>, now_secs: u64) {
    // Discover newly-exited sessions.
    for (key, session) in map.iter_mut() {
        session.poll_status();
        if session.is_terminal() {
            let _prev = exit_times.entry(key.clone()).or_insert(now_secs);
        }
    }

    // Remove sessions that have been exited long enough.
    let mut to_remove: Vec<String> = Vec::new();
    for (key, &first_seen) in exit_times.iter() {
        if now_secs.saturating_sub(first_seen) >= REAPER_GRACE_SECS {
            to_remove.push(key.clone());
        }
    }

    for key in &to_remove {
        if let Some(mut session) = map.remove(key) {
            drop(session.stdin.take());
        }
        let _prev = exit_times.remove(key);
    }

    // Clean exit_times for sessions that were manually removed.
    exit_times.retain(|k, _| map.contains_key(k));
}

// Here be the last port of call — once ye enter, no process leaves alive.
/// Kill all sessions — used during shutdown.
pub(crate) fn kill_all_sessions(sessions: &Sessions) {
    let mut map = sessions.lock().unwrap_or_else(PoisonError::into_inner);
    for session in map.values_mut() {
        if !session.is_terminal() {
            terminate(session.pid, 50);
        }
        drop(session.stdin.take());
    }
    map.clear();
}

/// SIGTERM `pid`, then SIGKILL it if still alive after `grace_ms`.
///
/// Polls liveness every 5 ms instead of sleeping the full grace period, so a
/// process that exits promptly (the common case) returns in a few ms. The
/// caller (and the agent blocked on the socket reply) no longer pays a fixed
/// delay. Worst case is unchanged: `grace_ms` then SIGKILL.
pub(crate) fn terminate(pid: u32, grace_ms: u64) {
    drop(Command::new("kill").args([&pid.to_string()]).output());
    let deadline = std::time::Instant::now().checked_add(std::time::Duration::from_millis(grace_ms));
    while is_pid_alive(pid) {
        if deadline.is_none_or(|d| std::time::Instant::now() >= d) {
            drop(Command::new("kill").args(["-9", &pid.to_string()]).output());
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
