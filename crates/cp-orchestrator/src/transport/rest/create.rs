//! `POST /api/fleet/create` — create a new agent and spawn it on a pty.
//!
//! Split out of [`rest`](super) for the 500-line file budget. Owns the
//! create-agent handler, its slug derivation, and the request/receipt JSON
//! shapes.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::{Backend, HttpReply};

/// `POST /api/fleet/create` — create a new agent and spawn it on a pty.
///
/// Body: `{ "name": "...", "folder": "...?", "model": "...?" }`. `name` is
/// required; `folder` defaults to `<agents_root>/<slug(name)>`; `model` is
/// accepted for forward-compat but not yet applied (the `cp` TUI has no
/// `--model` flag — a new agent boots with its folder's default model).
///
/// The flow: resolve + `mkdir -p` the realm folder, then ask the
/// [`AgentSupervisor`](crate::supervisor::AgentSupervisor) to spawn the `cp`
/// binary attached to a pty, with `CP_BRIDGE=1` and the backend's shared
/// `CP_AGENTS_DIR` so the agent self-registers where the backend scans. The
/// agent appears in the fleet within a scan tick once it has booted; the
/// receipt is therefore a 202-style "spawning" acknowledgement, not the agent
/// itself.
///
/// Returns `400` for a missing/blank name or malformed body, `502` for a
/// folder it cannot create or a spawn failure (incl. an off-allow-list
/// binary — which should never happen since the allow-list is seeded from the
/// configured binary).
pub fn create_agent(state: &Mutex<Backend>, body_bytes: &[u8]) -> HttpReply {
    let Ok(req) = serde_json::from_slice::<CreateAgentReq>(body_bytes) else {
        return HttpReply::error(400, "malformed create-agent request");
    };
    let name = req.name.trim();
    if name.is_empty() {
        return HttpReply::error(400, "agent name is required");
    }

    // Resolve the realm folder + the binary to spawn under the backend lock,
    // then release it before the (slower) filesystem + spawn work.
    let (folder, binary, agents_dir) = {
        let Ok(backend) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        let folder = match req.folder.as_deref() {
            Some(f) if !f.trim().is_empty() => std::path::PathBuf::from(f),
            _ => backend.agents_root.join(slugify(name)),
        };
        (folder, backend.agent_binary.clone(), backend.agents_dir.clone())
    };

    // Create the realm folder (idempotent).
    if let Err(e) = std::fs::create_dir_all(&folder) {
        return HttpReply::error(502, &format!("could not create realm folder: {e}"));
    }

    // Spawn on a pty under the supervisor's allow-list. The agent self-registers
    // into `agents_dir` (shared with the backend's scan) via `CP_BRIDGE=1`.
    let key = folder.to_string_lossy().into_owned();
    let agents_dir_str = agents_dir.to_string_lossy().into_owned();
    let env: [(&str, &str); 2] = [("CP_BRIDGE", "1"), ("CP_AGENTS_DIR", &agents_dir_str)];

    let spawn_result = {
        let Ok(mut backend) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        backend.supervisor.spawn_pty(key, &binary, &folder, &env)
    };

    match spawn_result {
        Ok(pid) => HttpReply::json(202, &CreateAgentReceipt {
            status: "spawning",
            folder: folder.to_string_lossy().into_owned(),
            pid,
        }),
        Err(e) => {
            eprintln!("create_agent spawn error: {e}");
            HttpReply::error(502, &format!("agent spawn failed: {e}"))
        }
    }
}

/// Derive a filesystem-safe realm slug from an agent name (mirrors the web
/// modal's `slugify`): lowercase, non-alphanumerics → `-`, trimmed, never empty.
fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut prev_dash = false;
    for ch in name.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() { "untitled".to_owned() } else { trimmed.to_owned() }
}

/// The `POST /api/fleet/create` request body.
///
/// A `model` field, if sent by the client, is silently ignored (serde does not
/// deny unknown fields) — the `cp` TUI has no `--model` flag yet, so a new
/// agent boots with its folder's default model.
#[derive(Deserialize)]
struct CreateAgentReq {
    /// Display name — the realm slug is derived from it when no folder is given.
    name: String,
    /// Explicit realm folder; when absent, `<agents_root>/<slug(name)>`.
    #[serde(default)]
    folder: Option<String>,
}

/// The receipt returned when an agent spawn has been launched.
#[derive(Serialize)]
struct CreateAgentReceipt {
    /// Always `"spawning"` — the agent appears in the fleet once it boots.
    status: &'static str,
    /// The realm folder the agent was spawned in.
    folder: String,
    /// The spawned process pid.
    pid: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_lowercases_and_dashes() {
        assert_eq!(slugify("My Project"), "my-project");
        assert_eq!(slugify("  Hello!!World  "), "hello-world");
        assert_eq!(slugify("a___b"), "a-b");
        assert_eq!(slugify("!!!"), "untitled");
        assert_eq!(slugify(""), "untitled");
    }
}
