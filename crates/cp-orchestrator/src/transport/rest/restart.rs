//! `POST /api/agent/{id}/restart` — kill a running agent and respawn it.
//!
//! Split out of [`rest`](super) for the 500-line file budget. The motivating
//! case: an agent whose **running binary is stale** (it predates a new command
//! the web cockpit wants to send), so its bridge socket rejects the command
//! with `502 agent unreachable`. Restarting respawns the agent from the
//! current [`agent_binary`](crate::transport::Backend) so the new command path
//! exists.
//!
//! The agent may have been launched **externally** (by hand, not by the
//! supervisor), so its pid is killed directly from the registry record rather
//! than through the supervised-child handle. Respawning on the **same folder**
//! re-registers it under the **same id** (the id is derived from the folder),
//! so the fleet sees the same agent come back to life.

use std::sync::Mutex;

use serde::Serialize;

use super::{Backend, HttpReply};
use crate::supervisor;

/// `POST /api/agent/{id}/restart` — terminate the agent's process and respawn
/// it on the same realm folder with the backend's current `cp` binary.
///
/// Flow:
/// 1. Resolve the registry entry → realm folder + running pid.
/// 2. Kill the old pid lock-free (SIGTERM → grace → SIGKILL); it may be an
///    externally-launched process the supervisor never owned.
/// 3. Drop any stale supervised record for this folder (reaps a spawned child).
/// 4. Respawn on the same folder via the supervisor pty path — the agent
///    self-registers under the same id within a scan tick.
///
/// Returns `202 {status:"restarting", folder, pid}` on success, `404` for an
/// unknown agent, `502` for a respawn failure.
pub fn restart_agent(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let entry = match super::resolve_entry(state, agent_id) {
        Ok(e) => e,
        Err(reply) => return reply,
    };
    let folder = std::path::PathBuf::from(&entry.folder);
    let key = folder.to_string_lossy().into_owned();

    // Snapshot what we need, then release the lock before the (slow) kill.
    let (binary, agents_dir, was_supervised) = {
        let Ok(backend) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        (backend.agent_binary.clone(), backend.agents_dir.clone(), backend.supervisor.is_supervised(&key))
    };

    // Kill the old process (lock-free — this can block up to the stop grace).
    supervisor::kill_pid(entry.pid);

    // Drop any stale supervised record so the respawn key is free. The pid is
    // already dead, so stop()'s grace loop returns immediately; a non-supervised
    // (external) agent yields NotFound, which we ignore.
    if was_supervised {
        let Ok(mut backend) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        let _stopped = backend.supervisor.stop(&key);
    }

    // Respawn on the same folder → re-registers under the same id.
    let agents_dir_str = agents_dir.to_string_lossy().into_owned();
    let env: [(&str, &str); 2] = [("CP_BRIDGE", "1"), ("CP_AGENTS_DIR", &agents_dir_str)];

    let spawn_result = {
        let Ok(mut backend) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        backend.supervisor.spawn_pty(key, &binary, &folder, &env)
    };

    match spawn_result {
        Ok(pid) => HttpReply::json(
            202,
            &RestartReceipt { status: "restarting", folder: folder.to_string_lossy().into_owned(), pid },
        ),
        Err(e) => {
            eprintln!("restart_agent spawn error: {e}");
            HttpReply::error(502, &format!("agent respawn failed: {e}"))
        }
    }
}

/// The receipt returned when an agent restart has been launched.
#[derive(Serialize)]
struct RestartReceipt {
    /// Always `"restarting"` — the agent re-appears in the fleet once it boots.
    status: &'static str,
    /// The realm folder the agent was respawned in.
    folder: String,
    /// The freshly spawned process pid.
    pid: u32,
}
