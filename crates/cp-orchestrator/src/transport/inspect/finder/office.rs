//! Office documents (`.docx` / `.xlsx` / `.pptx`) for the in-browser
//! `BetterOffice` editors, on top of [`cp_office`].
//!
//! The file on disk stays the source of truth: the browser fetches its bytes
//! (`GET fs/office`), polls `GET fs/office/version` to pick up an agent's edit,
//! and saves the whole file back (`POST fs/office?base=`), refused with `409`
//! when the file changed since the version it loaded.

use std::io::Read as _;
use std::path::PathBuf;
use std::sync::Mutex;

use cp_office::OfficeError;

use crate::transport::Backend;
use crate::transport::rest::HttpReply;

use super::support::{extract_param, fleet_folder, office_writable_path, previewable_path};

/// The `path` query param, required and non-empty.
fn path_param(query: &str) -> Result<String, HttpReply> {
    match extract_param(query, "path") {
        Some(p) if !p.is_empty() => Ok(p),
        _ => Err(HttpReply::error(400, "missing path parameter")),
    }
}

/// Map a [`cp_office`] refusal onto the HTTP reply the editor surfaces.
pub(super) fn reply_of(err: &OfficeError) -> HttpReply {
    HttpReply::error(err.status(), &err.to_string())
}

/// Resolve a readable document path (realm or read-only preview roots).
pub(super) fn readable(state: &Mutex<Backend>, fleet_id: &str, query: &str) -> Result<PathBuf, HttpReply> {
    let folder = fleet_folder(state, fleet_id)?;
    let relative = path_param(query)?;
    previewable_path(&folder, &relative).ok_or_else(|| HttpReply::error(403, "path outside agent realm"))
}

/// Resolve a writable document path (realm or `/tmp`, never the other read-only roots).
pub(super) fn confined_target(state: &Mutex<Backend>, fleet_id: &str, query: &str) -> Result<PathBuf, HttpReply> {
    let folder = fleet_folder(state, fleet_id)?;
    let relative = path_param(query)?;
    office_writable_path(&folder, &relative).ok_or_else(|| HttpReply::error(403, "path outside agent realm"))
}

/// `GET /api/fleet/{id}/fs/office?path=` — the document's bytes and version.
///
/// # Errors
///
/// `400` missing path, `403` outside the realm, then [`OfficeError::status`].
pub(crate) fn fs_office_read(
    state: &Mutex<Backend>,
    fleet_id: &str,
    query: &str,
) -> Result<cp_office::OfficeFile, HttpReply> {
    let target = readable(state, fleet_id, query)?;
    cp_office::read(&target).map_err(|e| reply_of(&e))
}

/// `GET /api/fleet/{id}/fs/office/version?path=` — `{ version }`, polled by the
/// editor to reload after an agent rewrote the file.
pub(crate) fn fs_office_version(state: &Mutex<Backend>, fleet_id: &str, query: &str) -> HttpReply {
    let target = match readable(state, fleet_id, query) {
        Ok(t) => t,
        Err(reply) => return reply,
    };
    match cp_office::version(&target) {
        Ok(version) => HttpReply::ok(&serde_json::json!({ "version": version })),
        Err(e) => reply_of(&e),
    }
}

/// `POST /api/fleet/{id}/fs/office?path=&base=` — body = the whole saved
/// document. Realm or `/tmp` (never the other read-only roots). Returns `{ version }`;
/// `409` when `base` no longer matches the file on disk.
pub(crate) fn fs_office_write(
    state: &Mutex<Backend>,
    fleet_id: &str,
    query: &str,
    body: &mut dyn std::io::Read,
) -> HttpReply {
    let folder = match fleet_folder(state, fleet_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    let relative = match path_param(query) {
        Ok(p) => p,
        Err(reply) => return reply,
    };
    let Some(target) = office_writable_path(&folder, &relative) else {
        return HttpReply::error(403, "path outside agent realm");
    };
    let mut bytes = Vec::new();
    if body.take(cp_office::MAX_BYTES.saturating_add(1)).read_to_end(&mut bytes).is_err() {
        return HttpReply::error(400, "could not read request body");
    }
    let base = extract_param(query, "base").filter(|b| !b.is_empty());
    match cp_office::write(&target, &bytes, base.as_deref()) {
        Ok(version) => HttpReply::ok(&serde_json::json!({ "version": version, "path": relative })),
        Err(e) => reply_of(&e),
    }
}
