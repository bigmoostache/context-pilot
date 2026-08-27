//! Process cleanup: reaper thread, graceful shutdown, signal handlers, FD limits.
//!
//! Extracted from `main.rs` to keep it under the 500-line limit.

use std::collections::BTreeMap;
use std::process::Command;
use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};

use crate::{SHUTDOWN_REQUESTED, Session, Sessions, is_pid_alive};

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
