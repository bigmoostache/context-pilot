//! `POST /api/fleet/create` — create a new agent and spawn it on a pty.
//!
//! Split out of [`rest`](super) for the 500-line file budget. Owns the
//! create-agent handler, its slug derivation, and the request/receipt JSON
//! shapes.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::{Backend, HttpReply};
use crate::services::auth::types::{AgentRole, User};

/// FNV-1a 64-bit offset basis (same constants as the agent-side identity
/// module in `cp-mod-bridge` — duplicated here to avoid a cross-crate dep
/// for 7 lines of pure hashing).
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Derive the agent registry id from a canonical folder path — the same
/// FNV-1a digest the agent mints at boot.
fn folder_id(path: &str) -> String {
    let mut hash = FNV_OFFSET;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

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
/// When auth is enabled the caller is automatically granted `agent-admin` on
/// the new agent, so the creator has immediate access without admin
/// intervention.
///
/// Returns `400` for a missing/blank name or malformed body, `502` for a
/// folder it cannot create or a spawn failure (incl. an off-allow-list
/// binary — which should never happen since the allow-list is seeded from the
/// configured binary).
pub fn create_agent(state: &Mutex<Backend>, body_bytes: &[u8], auth_user: Option<&User>) -> HttpReply {
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
        // Requirement 4 (T271): a folder still owned by a RETIRED agent must
        // not accept a fresh agent — the realm is reserved until unretired.
        if backend.retired.is_folder_retired(&folder.to_string_lossy()) {
            return HttpReply::error(409, "a retired agent owns this realm folder — unretire it instead");
        }
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
        Ok(pid) => {
            // Auto-grant the creator agent-admin access so they can
            // immediately see and manage the agent they just created.
            if let Some(user) = auth_user {
                if let Ok(b) = state.lock() {
                    if let Some(auth) = b.auth.as_ref() {
                        let canonical = folder.canonicalize().unwrap_or_else(|_| folder.clone());
                        let agent_id = folder_id(&canonical.to_string_lossy());
                        let _grant = auth.grant_access(&agent_id, &user.id, AgentRole::AgentAdmin, None);
                    }
                }
            }

            HttpReply::json(
                202,
                &CreateAgentReceipt { status: "spawning", folder: folder.to_string_lossy().into_owned(), pid },
            )
        }
        Err(e) => {
            eprintln!("create_agent spawn error: {e}");
            HttpReply::error(502, &format!("agent spawn failed: {e}"))
        }
    }
}

/// Derive a filesystem-safe realm slug from an agent name (mirrors the web
/// modal's `slugify`): lowercase, non-alphanumerics → `-`, trimmed, never empty.
///
/// `pub(super)` so the sibling command-create handler ([`super::library`]) can
/// reuse the exact same slug derivation when naming a `commands/<slug>.md` file.
pub(super) fn slugify(name: &str) -> String {
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
