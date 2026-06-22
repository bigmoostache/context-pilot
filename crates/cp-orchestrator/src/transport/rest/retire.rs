//! `POST /api/agent/{id}/retire` + `/unretire` — stop-and-keep an agent, or
//! bring it back (T271).
//!
//! Split out of [`rest`](super) for the 500-line file budget. Retiring an agent
//! is the dashboard's "archive" action, deliberately **not** a delete:
//!
//! * the realm **folder is left untouched**;
//! * the agent's **Rust process is killed** (lock-free, like
//!   [`restart`](super::restart)), and so is its **console-server daemon**
//!   (which is designed to survive a TUI restart, so it must be stopped
//!   explicitly via its `server.pid` file);
//! * the retired state is recorded in the orchestrator-owned
//!   [`RetiredStore`](crate::retire::RetiredStore) — **not** the agent's
//!   registry record, which a clean shutdown removes — so the Retired card can
//!   be rendered with no live process to inspect, and a same-path create can be
//!   blocked.
//!
//! Unretiring removes the flag and **respawns** the agent on the same folder
//! (re-registering under the same id), symmetric with retire's kill and reusing
//! the supervisor pty path.

use std::sync::Mutex;

use serde::Serialize;

use super::{Backend, HttpReply};
use crate::services::RetiredRecord;
use crate::supervisor;

/// `POST /api/agent/{id}/retire` — stop the agent (process + console server),
/// keep its folder, and record it as retired.
///
/// Flow:
/// 1. Resolve the registry entry → realm folder, pid, model.
/// 2. Snapshot the display info (name, model, provider) into a
///    [`RetiredRecord`] *before* killing anything — the registry record may
///    vanish on the agent's clean shutdown.
/// 3. Kill the agent process lock-free (SIGTERM → grace → SIGKILL); drop any
///    stale supervised record.
/// 4. Kill the console-server daemon via `<folder>/.context-pilot/console/
///    server.pid` (best-effort — it may already be gone).
/// 5. Mark retired in the store (persisted).
///
/// Returns `200 {status:"retired", id, folder}` on success, `404` for an
/// unknown agent.
pub fn retire_agent(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let entry = match super::resolve_entry(state, agent_id) {
        Ok(e) => e,
        Err(reply) => return reply,
    };
    let folder = entry.folder.clone();
    let key = std::path::PathBuf::from(&folder).to_string_lossy().into_owned();
    let name =
        std::path::Path::new(&folder).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or(agent_id).to_owned();

    // Provider snapshot (best-effort) before the process dies.
    let provider = crate::transport::inspect::meta::read_provider(state, &folder);

    let record = RetiredRecord {
        id: agent_id.to_owned(),
        name,
        folder: folder.clone(),
        model: entry.model.clone(),
        provider,
        retired_at_ms: now_ms(),
    };

    // Was the agent supervised? (Snapshot, then release the lock before kills.)
    let was_supervised = {
        let Ok(b) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        b.supervisor.is_supervised(&key)
    };

    // Kill the agent process (lock-free — can block up to the stop grace).
    supervisor::kill_pid(entry.pid);

    // Kill the console-server daemon (best-effort; it survives TUI restarts by
    // design, so retiring the agent does not take it down on its own).
    kill_console_server(&folder);

    // Drop any stale supervised record so a later unretire respawn key is free.
    if was_supervised {
        if let Ok(mut b) = state.lock() {
            let _stopped = b.supervisor.stop(&key);
        }
    }

    // Record retired (persisted).
    {
        let Ok(mut b) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        b.retired.retire(record);
    }

    HttpReply::json(200, &RetireReceipt { status: "retired", id: agent_id, folder })
}

/// `POST /api/agent/{id}/unretire` — clear the retired flag and respawn the
/// agent on its kept folder.
///
/// Returns `202 {status:"unretiring", id, folder, pid}` on success, `404` if
/// the agent is not retired, `502` for a respawn failure.
pub fn unretire_agent(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    // Clear the flag, recovering the snapshot (404 if it was never retired).
    let record = {
        let Ok(mut b) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        match b.retired.unretire(agent_id) {
            Some(r) => r,
            None => return HttpReply::error(404, "agent is not retired"),
        }
    };

    let folder = std::path::PathBuf::from(&record.folder);
    let key = folder.to_string_lossy().into_owned();

    // Respawn on the same folder → re-registers under the same id.
    let (binary, agents_dir) = {
        let Ok(b) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        (b.agent_binary.clone(), b.agents_dir.clone())
    };
    let agents_dir_str = agents_dir.to_string_lossy().into_owned();
    let env: [(&str, &str); 2] = [("CP_BRIDGE", "1"), ("CP_AGENTS_DIR", &agents_dir_str)];

    let spawn_result = {
        let Ok(mut b) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        b.supervisor.spawn_pty(key, &binary, &folder, &env)
    };

    match spawn_result {
        Ok(pid) => {
            HttpReply::json(202, &UnretireReceipt { status: "unretiring", id: agent_id, folder: record.folder, pid })
        }
        Err(e) => {
            eprintln!("unretire_agent spawn error: {e}");
            HttpReply::error(502, &format!("agent respawn failed: {e}"))
        }
    }
}

/// Kill an agent's console-server daemon via its pid file.
///
/// The console server (`cp-console-server`) writes its pid to
/// `<folder>/.context-pilot/console/server.pid` and is built to **survive** a
/// TUI restart, so killing the agent does not stop it. This reads that pid and
/// signals it (SIGTERM → grace → SIGKILL via [`supervisor::kill_pid`]) so the
/// daemon shuts its child sessions down cleanly. Entirely best-effort: a
/// missing/garbage file (no console ever started) is a silent no-op.
fn kill_console_server(folder: &str) {
    let pid_path = std::path::Path::new(folder).join(".context-pilot").join("console").join("server.pid");
    let Ok(raw) = std::fs::read_to_string(&pid_path) else {
        return;
    };
    if let Ok(pid) = raw.trim().parse::<u32>() {
        supervisor::kill_pid(pid);
    }
}

/// Wall-clock epoch-ms (saturating to 0 before the epoch — impossible here).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// The receipt returned when an agent has been retired.
#[derive(Serialize)]
struct RetireReceipt<'a> {
    /// Always `"retired"`.
    status: &'static str,
    /// The agent id retired.
    id: &'a str,
    /// The realm folder, kept intact.
    folder: String,
}

/// The receipt returned when an agent unretire (respawn) has been launched.
#[derive(Serialize)]
struct UnretireReceipt<'a> {
    /// Always `"unretiring"` — the agent re-appears in the active fleet once it
    /// boots.
    status: &'static str,
    /// The agent id being brought back.
    id: &'a str,
    /// The realm folder it was respawned in.
    folder: String,
    /// The freshly spawned process pid.
    pid: u32,
}
