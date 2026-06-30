//! `POST /api/fleet/create` + `POST /api/agent/{id}/library/command` —
//! create agents and prompt-library commands.
//!
//! Split out of [`rest`](super) for the 500-line file budget. Owns the
//! create-agent handler, its slug derivation, the create-command handler,
//! and the request/receipt JSON shapes.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::{Backend, HttpReply, resolve_entry};
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

    #[test]
    fn yaml_scalar_quotes_and_escapes() {
        assert_eq!(yaml_scalar("Hello"), "\"Hello\"");
        assert_eq!(yaml_scalar("a \"b\" c"), "\"a \\\"b\\\" c\"");
        assert_eq!(yaml_scalar("line1\nline2"), "\"line1 line2\"");
        assert_eq!(yaml_scalar("back\\slash"), "\"back\\\\slash\"");
    }
}

// ── Create command (prompt library) ─────────────────────────────────

/// `POST /api/agent/{id}/library/command` — write a new command markdown file.
///
/// Body: `{ "name": "...", "description": "...?", "body": "..." }`. `name` and
/// `body` are required (the slug is derived from `name`, the body is the prompt
/// the `/command` expands to); `description` is optional (the one-line label
/// shown on the suggestion bubble).
///
/// Returns `201` with `{ "id": <slug>, "status": "created" }` on success,
/// `400` for a missing/blank name or body or malformed JSON, `404` for an
/// unknown agent, `409` when a command with that slug already exists (never
/// clobbers), and `502` if the file cannot be written.
pub fn create_command(state: &Mutex<Backend>, id: &str, body_bytes: &[u8]) -> HttpReply {
    let Ok(req) = serde_json::from_slice::<CreateCommandReq>(body_bytes) else {
        return HttpReply::error(400, "malformed create-command request");
    };
    let name = req.name.trim();
    if name.is_empty() {
        return HttpReply::error(400, "command name is required");
    }
    let body = req.body.trim();
    if body.is_empty() {
        return HttpReply::error(400, "command body is required");
    }

    let entry = match resolve_entry(state, id) {
        Ok(e) => e,
        Err(reply) => return reply,
    };

    let slug = slugify(name);
    let commands_dir = std::path::Path::new(&entry.folder).join(".context-pilot").join("commands");
    let file_path = commands_dir.join(format!("{slug}.md"));

    if file_path.exists() {
        return HttpReply::error(409, "a command with this name already exists");
    }

    if let Err(e) = std::fs::create_dir_all(&commands_dir) {
        return HttpReply::error(502, &format!("could not create commands directory: {e}"));
    }

    let description = req.description.trim();
    let mut markdown = String::new();
    markdown.push_str("---\n");
    markdown.push_str(&format!("name: {}\n", yaml_scalar(name)));
    markdown.push_str(&format!("description: {}\n", yaml_scalar(description)));
    markdown.push_str("---\n");
    markdown.push_str(body);
    markdown.push('\n');

    if let Err(e) = std::fs::write(&file_path, markdown) {
        return HttpReply::error(502, &format!("could not write command file: {e}"));
    }

    HttpReply::json(201, &CreateCommandReceipt { id: slug, status: "created" })
}

/// Encode a single-line string as a double-quoted YAML scalar.
///
/// Backslashes and double quotes are escaped, and any CR/LF is collapsed to a
/// space so the value stays on one frontmatter line.
fn yaml_scalar(s: &str) -> String {
    let mut out = String::with_capacity(s.len().saturating_add(2));
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\r' | '\n' => out.push(' '),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// The `POST /api/agent/{id}/library/command` request body.
#[derive(Deserialize)]
struct CreateCommandReq {
    name: String,
    #[serde(default)]
    description: String,
    body: String,
}

/// The receipt returned when a command file has been created.
#[derive(Serialize)]
struct CreateCommandReceipt {
    id: String,
    status: &'static str,
}
