//! Per-agent **public profile** endpoints — display name and profile picture.
//!
//! Split out of [`rest`](crate::transport::rest) when the T739b rewire pushed
//! that module past the 500-line cap. The seam is the natural one: `rest`
//! keeps the fleet/agent/command surface, while the profile a *human* sees for
//! an agent lives here, beside the other per-agent configuration handlers.
//!
//! # Ownership (T739b)
//!
//! These three handlers used to write orchestrator-side sidecars
//! (`agent-names.json`, `agent-avatars/`) that the agent could neither see nor
//! set. The agent now **owns** both values: each handler sends a
//! [`SetProfile`](cp_wire::types::command::Kind::SetProfile) command, the
//! agent's shared Agora core validates and applies it, and the agent's own
//! registry projection re-advertises it. One authority, one value, no sidecar
//! that can drift.
//!
//! Two properties are load-bearing across all three:
//!
//! * **Patch shape.** Every call sets exactly one field and passes `None` for
//!   the other, so a rename can never clear an avatar and an avatar upload can
//!   never clear a name.
//! * **Lossless fallback.** A dead or never-booted agent has no process to
//!   command, so the legacy sidecar is still written in that case (and still
//!   read as the fallback on the read path). This is what lets the migration
//!   proceed without stranding a stopped agent's name.

use std::sync::Mutex;

use super::super::{Backend, HttpReply, resolve_entry};
use crate::services::agent_meta;

/// `POST /api/agent/{id}/rename` — set or clear a custom display name.
///
/// Body: `{ "name": "My Custom Name" }`. An empty or whitespace-only name
/// clears the override, reverting to the folder-derived default.
///
/// `image: None` keeps the patch honest — renaming must not disturb an avatar
/// the same agent already advertises.
pub(crate) fn rename_agent(state: &Mutex<Backend>, id: &str, body_bytes: &[u8]) -> HttpReply {
    #[derive(serde::Deserialize)]
    struct Req {
        /// Requested display name; empty or whitespace-only clears it.
        name: String,
    }
    let Ok(req) = serde_json::from_slice::<Req>(body_bytes) else {
        return HttpReply::error(400, "expected {\"name\":\"...\"}");
    };

    // Preferred path: the agent owns the value.
    if let Ok(entry) = resolve_entry(state, id)
        && agent_meta::send_profile(&entry, Some(req.name.trim().to_owned()), None).is_ok()
    {
        if let Ok(mut b) = state.lock() {
            b.mark_dirty(id);
        }
        return HttpReply::ok(&serde_json::json!({ "ok": true }));
    }

    // Fallback: a dead agent cannot be commanded, so keep the legacy override.
    let Ok(mut b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let _prev = b.names.set(id, &req.name);
    HttpReply::ok(&serde_json::json!({ "ok": true }))
}

/// `POST /api/agent/{id}/avatar` — upload or replace an agent's profile picture.
///
/// Body: raw image bytes (PNG/JPEG/GIF/WebP/SVG). The content type is sniffed
/// from magic bytes, so no `Content-Type` header is required. Max 2 MiB
/// ([`MAX_AVATAR_BYTES`](crate::services::agent_meta::MAX_AVATAR_BYTES)).
///
/// The **bytes** remain an orchestrator concern — it owns the only HTTP
/// surface, so an upload has to land here — but the **reference** is
/// agent-owned: the image is written into the agent's own realm and only its
/// realm-relative path travels on in the command. A multi-megabyte blob never
/// enters the `0600` registry record, which is re-read on every fleet scan.
///
/// A rejected image (too large, not an image) is the *user's* error and is
/// reported as a `400`, deliberately **not** falling through to the legacy
/// store — only an unreachable agent triggers the fallback.
pub(crate) fn upload_avatar(state: &Mutex<Backend>, id: &str, body_bytes: &[u8]) -> HttpReply {
    if body_bytes.is_empty() {
        return HttpReply::error(400, "empty body");
    }

    // Preferred path: write into the agent's realm, hand it the reference.
    if let Ok(entry) = resolve_entry(state, id) {
        match agent_meta::write_avatar_into_realm(&entry.folder, body_bytes) {
            Ok(reference) => {
                if agent_meta::send_profile(&entry, None, Some(reference)).is_ok() {
                    if let Ok(mut b) = state.lock() {
                        b.mark_dirty(id);
                    }
                    return HttpReply::ok(&serde_json::json!({ "ok": true }));
                }
            }
            Err(msg) => return HttpReply::error(400, &msg),
        }
    }

    // Fallback: a dead agent cannot be commanded, so keep the legacy store.
    let Ok(mut b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    match b.avatars.set(id, body_bytes) {
        Ok(()) => HttpReply::ok(&serde_json::json!({ "ok": true })),
        Err(msg) => HttpReply::error(400, &msg),
    }
}

/// `DELETE /api/agent/{id}/avatar` — remove an agent's profile picture.
///
/// Clears the agent-owned reference with `image: Some("")` — a deliberate
/// clear, distinct from `None`, which would leave it untouched — and deletes
/// the bytes from the realm.
///
/// The legacy sidecar entry is dropped **unconditionally**, even when the agent
/// was commanded successfully: otherwise a pre-migration avatar would survive
/// the delete and resurface through the read path's fallback.
pub(crate) fn delete_avatar(state: &Mutex<Backend>, id: &str) -> HttpReply {
    if let Ok(entry) = resolve_entry(state, id) {
        agent_meta::remove_realm_avatars(&entry.folder);
        let _sent = agent_meta::send_profile(&entry, None, Some(String::new()));
    }
    let Ok(mut b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let _existed = b.avatars.remove(id);
    b.mark_dirty(id);
    HttpReply::ok(&serde_json::json!({ "ok": true }))
}
