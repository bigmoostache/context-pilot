//! REST resource handlers — the request/response half of the transport.
//!
//! Every read response is wrapped in an [`Envelope`] carrying the `rev` it
//! reflects, so a client always knows how current its data is and can resume an
//! SSE stream from exactly that point. Actions (`POST /command`) return a
//! [`CommandReceipt`] with the `cmd_id`, the caller's `dedup_token`, and the
//! durable `rev` the effect landed at — everything a client needs to retry
//! safely (the agent's seen-set deduplicates a replay).
//!
//! Handlers return a transport-agnostic [`HttpReply`] (status + JSON body); the
//! server module turns that into a concrete `tiny_http` response. This keeps
//! the handlers pure and unit-testable without a live socket.
//!
//! Concurrency: each handler locks the shared [`Backend`](super::Backend) only
//! briefly to read projected state, and **never** holds the lock across the
//! blocking agent I/O of a body hydrate or command send.

use std::sync::Mutex;

use cp_wire::types::ContentHash;
use cp_wire::types::command::Command;
use cp_wire::types::registry::Entry;
use serde::Serialize;

use crate::channel::AgentChannel;

mod backend;
mod claude_oauth;
mod config;
mod create;
mod lifecycle;
mod releases;
mod thread_shape;
pub use backend::Backend;
pub(crate) use claude_oauth::accounts::{delete_account, list_accounts, store_account, switch_account};
pub(crate) use claude_oauth::{claude_usage, login_complete, login_start, refresh_login, token_status};
pub(crate) use config::env_keys::{env_key_reveal, env_key_update, env_keys_list, vault_snapshot};
pub(crate) use config::it::{it_ca_fingerprint, it_get_identity, it_provisioned, it_set_identity};
pub(crate) use config::settings::{allowed_models, onboarding_completed};
pub use config::settings::{get_settings, update_settings};
pub(crate) use config::update::{APPLY_IN_FLIGHT, update_apply, update_check, update_set_mode, update_status};
pub use create::{create_agent, create_command};
pub use lifecycle::{restart_agent, retire_agent, unretire_agent};
pub(crate) use releases::{
    delete_release, deploy_fleet, download_release, list_releases, releases_break_glass, restart_orchestrator,
    select_release, set_arch,
};
use thread_shape::{overlay_roster, reshape_thread};

/// A transport-agnostic reply: an HTTP status and a JSON body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpReply {
    /// HTTP status code.
    pub status: u16,
    /// JSON-encoded response body.
    pub body: String,
}

impl HttpReply {
    /// A `200 OK` carrying `value` serialized as JSON.
    pub(crate) fn ok<T: Serialize>(value: &T) -> Self {
        Self::json(200, value)
    }

    /// A reply with `status` carrying `value` serialized as JSON. Serialization
    /// is infallible for our own types; a failure degrades to a `500`.
    fn json<T: Serialize>(status: u16, value: &T) -> Self {
        match serde_json::to_string(value) {
            Ok(body) => Self { status, body },
            Err(_) => Self::error(500, "serialization failed"),
        }
    }

    /// An error reply with a `{"error": reason}` body.
    pub(crate) fn error(status: u16, reason: &str) -> Self {
        Self { status, body: format!("{{\"error\":{}}}", json_string(reason)) }
    }
}

/// A read response wrapping its payload with the `rev` it reflects.
#[derive(Debug, Serialize)]
pub struct Envelope<T: Serialize> {
    /// The oplog `rev` this payload reflects — an SSE stream can resume here.
    pub rev: u64,
    /// The resource payload.
    pub data: T,
}

/// The receipt returned for an accepted (or deduplicated) command.
#[derive(Debug, Serialize)]
pub struct CommandReceipt {
    /// Transport-level command id echoed from the agent's ack.
    pub cmd_id: String,
    /// The caller's semantic dedup token — replay-safe retry key.
    pub dedup_token: String,
    /// Durable `rev` the effect landed at, if accepted.
    pub rev: Option<u64>,
    /// `"accepted"` or `"rejected"`.
    pub status: String,
}

/// `GET /api/fleet` — every known agent's projected view.
///
/// The envelope `rev` is the maximum `rev` across all agents, so a client can
/// open one fleet-wide stream from that point.
///
/// When auth is enabled, the result is filtered to only agents the caller has
/// access to (FR-12). System admins see everything (FR-09).
pub fn fleet(state: &Mutex<Backend>, auth_user: Option<&crate::services::auth::types::User>) -> HttpReply {
    let Ok(backend) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let all_ids: Vec<String> = backend.view.agent_ids().map(str::to_owned).collect();
    drop(backend);

    // Filter by ACL when auth is enabled (FR-12).
    let visible_ids = crate::transport::auth::filter_fleet(state, &all_ids, auth_user);

    let Ok(backend) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let mut agents = serde_json::Map::new();
    let mut max_rev = 0u64;
    for id in &visible_ids {
        if let Some(view) = backend.view.get(id) {
            max_rev = max_rev.max(view.rev);
            if let Ok(value) = serde_json::to_value(view) {
                let _prev = agents.insert(id.clone(), value);
            }
        }
    }
    HttpReply::ok(&Envelope { rev: max_rev, data: agents })
}

/// `GET /api/agent/{id}` — one agent's projected view, or `404`.
pub fn agent(state: &Mutex<Backend>, id: &str) -> HttpReply {
    let Ok(backend) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    match backend.view.get(id) {
        Some(view) => HttpReply::ok(&Envelope { rev: view.rev, data: view }),
        None => HttpReply::error(404, "unknown agent"),
    }
}

/// `POST /api/ticket` — mint a single-use SSE upgrade ticket.
///
/// When auth is enabled the caller's user id is embedded in the ticket so the
/// SSE redeem path can enforce per-agent ACL (Phase 7, design doc §7).
pub fn mint_ticket(state: &Mutex<Backend>, auth_user: Option<&crate::services::auth::types::User>) -> HttpReply {
    let Ok(mut backend) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let token = backend.tickets.mint(auth_user.map(|u| u.id.as_str()));
    HttpReply { status: 200, body: format!("{{\"ticket\":{}}}", json_string(&token)) }
}

/// `GET /api/agent/{id}/body/{hash}` — hydrate a content-addressed body.
///
/// Returns the raw bytes as a JSON string-free octet payload on success, `400`
/// for a malformed hash, `404` for an unknown agent or absent/inlined body, and
/// `502` if the agent's store cannot be read.
pub fn body(state: &Mutex<Backend>, id: &str, hash_hex: &str) -> HttpReply {
    let Some(hash) = ContentHash::from_hex(hash_hex) else {
        return HttpReply::error(400, "malformed content hash");
    };
    let entry = match resolve_entry(state, id) {
        Ok(entry) => entry,
        Err(reply) => return reply,
    };
    // Hydrate is blocking agent I/O — performed with no lock held.
    match AgentChannel::from_entry(&entry).hydrate(hash) {
        Ok(Some(bytes)) => {
            HttpReply { status: 200, body: serde_json::to_string(&BodyPayload { bytes: &bytes }).unwrap_or_default() }
        }
        Ok(None) => HttpReply::error(404, "body not found"),
        Err(_) => HttpReply::error(502, "body read failed"),
    }
}

/// `POST /api/agent/{id}/command` — submit a command to an agent.
///
/// Returns `400` for a malformed command, `404` for an unknown agent, and `502`
/// if the agent is unreachable.
pub fn command(state: &Mutex<Backend>, id: &str, body_bytes: &[u8]) -> HttpReply {
    let Ok(command) = serde_json::from_slice::<Command>(body_bytes) else {
        return HttpReply::error(400, "malformed command");
    };

    let entry = match resolve_entry(state, id) {
        Ok(entry) => entry,
        Err(reply) => return reply,
    };
    let dedup_token = command.dedup_token.clone();
    match AgentChannel::from_entry(&entry).send(command) {
        Ok(ack) => {
            // Mark state dirty so SSE producers emit an `invalidate` event,
            // prompting connected frontends to refetch tier-② data.
            if let Ok(mut b) = state.lock() {
                b.mark_dirty(id);
            }
            HttpReply::ok(&CommandReceipt {
                cmd_id: ack.cmd_id,
                dedup_token,
                rev: ack.rev,
                status: ack_status(&ack.status),
            })
        }
        Err(e) => {
            eprintln!("command send error for agent {id}: {e:?}");
            HttpReply::error(502, "agent unreachable")
        }
    }
}

/// `POST /api/agent/{id}/rename` — set or clear a custom display name.
///
/// Body: `{ "name": "My Custom Name" }`.  An empty or whitespace-only name
/// reverts to the folder-derived default.  The override is persisted in the
/// orchestrator's `agent-names.json` (independent of the agent process).
pub fn rename_agent(state: &Mutex<Backend>, id: &str, body_bytes: &[u8]) -> HttpReply {
    #[derive(serde::Deserialize)]
    struct Req {
        name: String,
    }
    let Ok(req) = serde_json::from_slice::<Req>(body_bytes) else {
        return HttpReply::error(400, "expected {\"name\":\"...\"}");
    };
    let Ok(mut b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let _prev = b.names.set(id, &req.name);
    HttpReply::ok(&serde_json::json!({ "ok": true }))
}

/// `POST /api/agent/{id}/avatar` — upload or replace an agent's profile picture.
///
/// Body: raw image bytes (PNG/JPEG/GIF/WebP/SVG). Content type is sniffed from
/// magic bytes — the `Content-Type` header is not required. Max 2 MiB
/// ([`MAX_AVATAR_BYTES`](crate::services::agent_meta::MAX_AVATAR_BYTES)).
pub fn upload_avatar(state: &Mutex<Backend>, id: &str, body_bytes: &[u8]) -> HttpReply {
    if body_bytes.is_empty() {
        return HttpReply::error(400, "empty body");
    }
    let Ok(mut b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    match b.avatars.set(id, body_bytes) {
        Ok(()) => HttpReply::ok(&serde_json::json!({ "ok": true })),
        Err(msg) => HttpReply::error(400, &msg),
    }
}

/// `DELETE /api/agent/{id}/avatar` — remove an agent's profile picture.
pub fn delete_avatar(state: &Mutex<Backend>, id: &str) -> HttpReply {
    let Ok(mut b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let _existed = b.avatars.remove(id);
    HttpReply::ok(&serde_json::json!({ "ok": true }))
}

/// Load an agent's registry [`Entry`] from the configured agents directory.
///
/// Returns an [`HttpReply`] error directly so handlers can `?`-style early-out.
pub(super) fn resolve_entry(state: &Mutex<Backend>, id: &str) -> Result<Entry, HttpReply> {
    let dir = {
        let backend = state.lock().map_err(|_| HttpReply::error(500, "backend lock poisoned"))?;
        backend.agents_dir.clone()
    };
    let path = dir.join(format!("{id}.json"));
    let raw = std::fs::read(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => HttpReply::error(404, "unknown agent"),
        _ => HttpReply::error(502, "registry read failed"),
    })?;
    serde_json::from_slice::<Entry>(&raw).map_err(|_| HttpReply::error(502, "registry record corrupt"))
}

/// Map an [`Ack`](cp_wire::types::ack::Ack) status to a short string.
fn ack_status(status: &cp_wire::types::ack::Status) -> String {
    match status {
        cp_wire::types::ack::Status::Accepted => "accepted".to_owned(),
        cp_wire::types::ack::Status::Rejected { .. } => "rejected".to_owned(),
    }
}

/// `GET /api/agent/{id}/threads` — the thread list, served roster-first.
///
/// The thread **roster** (which threads exist, their turn status, archived
/// flag, and last activity) comes from the in-memory
/// [`MaterializedView`](crate::services::MaterializedView) — folded live from
/// the agent's oplog, so a just-created/archived/restored thread is reflected
/// in milliseconds, never waiting on the debounced tier-② disk write (design
/// doc I5: live reads ride the view, not disk).
///
/// The per-thread **message log** is hydrated best-effort from the agent's
/// `config.json` (via [`StateReader`](crate::inspect::StateReader)): the view
/// roster carries message *counts* but not bodies, and the conversation log is
/// only needed once a thread is opened. The two are merged by thread id:
///
/// * a thread present in both — disk supplies its `log`, the view supplies the
///   fresher `status` / `archived` / `lastActivity` (the disk copy may lag);
/// * a thread only in the view (created after the last disk flush) — synthesised
///   from the roster alone with an empty log (it gains its log on the next disk
///   flush, and later via live `MessageCreated` deltas);
/// * a thread only on disk (created before roster journaling began) — kept
///   verbatim from disk.
///
/// This makes thread *appearance* instant (the user's latency complaint) while
/// keeping conversations intact during the migration to a fully delta-driven
/// read path.
pub fn threads(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let entry = match resolve_entry(state, agent_id) {
        Ok(e) => e,
        Err(reply) => return reply,
    };
    let folder = std::path::Path::new(&entry.folder);
    let (config, focused_thread_id, roster) = {
        let Ok(mut b) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        let reader = b.inspect_mut();
        let cfg = reader.read_config(folder).ok();

        // Read focused_thread_id from the first worker's FocusState.
        let focused = reader.list_workers(folder).unwrap_or_default().into_iter().find_map(|wid| {
            reader.read_worker(folder, &wid).ok().and_then(|w| {
                w.get("modules")
                    .and_then(|m| m.get("threads_worker"))
                    .and_then(|tw| tw.get("focused_thread_id"))
                    .and_then(serde_json::Value::as_str)
                    .map(String::from)
            })
        });
        // The `reader` borrow ends with `cfg`/`focused` (both owned); now read
        // the live roster + focused thread from the view under the same lock.
        // The view's focus (push-fed via `ThreadFocusChanged`) is the fresher
        // authority; fall back to the disk `FocusState` only when the view has
        // none yet (a backend cold start before the agent's first focus
        // emission — design doc I5: live reads ride the view, disk is the
        // bounded backstop).
        let (roster, view_focus) =
            b.view.get(agent_id).map(|v| (v.roster.clone(), v.focused_thread_id.clone())).unwrap_or_default();
        let focused = view_focus.or(focused);
        (cfg, focused, roster)
    };

    // Disk threads (full logs). Absent config is tolerated: the view roster
    // alone can still render newly-created threads.
    let empty_arr = serde_json::Value::Array(Vec::new());
    let raw_threads = config
        .as_ref()
        .and_then(|c| c.get("modules"))
        .and_then(|m| m.get("threads"))
        .and_then(|t| t.get("threads"))
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| empty_arr.as_array().expect("empty vec is array"));

    let mut details: Vec<serde_json::Value> = raw_threads.iter().map(|t| reshape_thread(t, agent_id)).collect();

    // Overlay the view's fresher roster onto matching disk threads, then append
    // any view-only threads the disk has not yet flushed.
    overlay_roster(&mut details, &roster, agent_id);

    HttpReply::ok(&serde_json::json!({
        "focusedThreadId": focused_thread_id,
        "threads": details,
    }))
}

/// JSON-encode a string (with surrounding quotes and escaping).
fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_owned())
}

/// The JSON body for a hydrated body: bytes serialized as a number array.
#[derive(Serialize)]
struct BodyPayload<'a> {
    /// Raw body bytes.
    bytes: &'a [u8],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::MaterializedView;
    use cp_wire::types::Phase;
    use cp_wire::types::oplog::{OpEntry, OpEntryKind};
    use std::path::PathBuf;

    fn phase_entry(rev: u64, phase: Phase) -> OpEntry {
        OpEntry::new(1, rev, 0, OpEntryKind::PhaseTransition { phase })
    }

    fn backend_with_agent() -> Mutex<Backend> {
        let mut view = MaterializedView::new();
        view.apply("a1", &phase_entry(7, Phase::Streaming));
        Mutex::new(Backend::for_test(PathBuf::from("/tmp/cp-test-agents"), view))
    }

    #[test]
    fn fleet_lists_agents_with_max_rev() {
        let state = backend_with_agent();
        let reply = fleet(&state, None);
        assert_eq!(reply.status, 200);
        assert!(reply.body.contains("\"rev\":7"), "envelope carries the max rev");
        assert!(reply.body.contains("a1"), "agent id present");
    }

    #[test]
    fn agent_returns_view_with_rev() {
        let state = backend_with_agent();
        let reply = agent(&state, "a1");
        assert_eq!(reply.status, 200);
        assert!(reply.body.contains("\"rev\":7"));
    }

    #[test]
    fn agent_unknown_is_404() {
        let state = backend_with_agent();
        assert_eq!(agent(&state, "ghost").status, 404);
    }

    #[test]
    fn mint_ticket_returns_token() {
        let state = backend_with_agent();
        let reply = mint_ticket(&state, None);
        assert_eq!(reply.status, 200);
        assert!(reply.body.contains("\"ticket\""));
    }

    #[test]
    fn body_rejects_malformed_hash() {
        let state = backend_with_agent();
        assert_eq!(body(&state, "a1", "not-a-hash").status, 400);
    }

    #[test]
    fn command_rejects_malformed_body() {
        let state = backend_with_agent();
        assert_eq!(command(&state, "a1", b"{not json").status, 400);
    }
}
