//! File-upload write handlers for the Finder, split from [`super::mutate`] to
//! keep each file within the 500-line budget. Both endpoints write a single
//! uploaded file's raw body into the agent realm, confined and collision-aware;
//! they share the bare-name and realm-relative-path helpers with `mutate` via
//! `super::{is_bare_name, rel_child}`.

use std::sync::Mutex;

use crate::transport::Backend;
use crate::transport::rest::HttpReply;

use super::support::{agent_folder, confined_path, extract_param};
use super::{is_bare_name, rel_child};

/// `POST /api/agent/{id}/fs/upload?path={dir}&name={file}` — upload one file.
///
/// The request body is the file's raw bytes (the transport caps it at
/// `MAX_BODY`); `path` is the confined destination directory (empty = realm
/// root) and `name` is the bare destination filename. One file per request —
/// the Finder fires N concurrent uploads for a multi-file selection, sidestepping
/// multipart parsing entirely.
///
/// The destination directory is confined to the agent realm (no `..`/symlink
/// escape) and must already exist; `name` must be a bare filename (no path
/// separators, not `.`/`..`) so the write can never land outside the realm.
/// Returns `{ written, path }` (bytes written + the realm-relative path).
pub fn fs_upload(state: &Mutex<Backend>, agent_id: &str, query: &str, body: &[u8]) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    let relative_dir = extract_param(query, "path").unwrap_or_default();
    let name = match extract_param(query, "name") {
        Some(n) if !n.is_empty() => n,
        _ => return HttpReply::error(400, "missing name parameter"),
    };

    // The filename must be a bare component — a separator or `..` would let the
    // write escape the confined directory.
    if !is_bare_name(&name) {
        return HttpReply::error(400, "invalid file name");
    }

    let dir = match confined_path(&folder, &relative_dir) {
        Some(p) => p,
        None => return HttpReply::error(403, "path outside agent realm"),
    };
    if !dir.is_dir() {
        return HttpReply::error(404, "destination directory not found");
    }

    let dest = dir.join(&name);
    if std::fs::write(&dest, body).is_err() {
        return HttpReply::error(502, "write failed");
    }

    HttpReply::ok(&serde_json::json!({
        "written": body.len(),
        "path": rel_child(&relative_dir, &name),
    }))
}

/// `POST /api/agent/{id}/fs/upload-unique?path={dir}&name={name}` — upload a
/// file, auto-creating the destination directory and never overwriting.
///
/// Like [`fs_upload`], the body is the file's raw bytes and `name` is the bare
/// destination filename — but this variant is tailored to the chat composer's
/// attachment flow:
///
/// * The destination directory `path` (e.g. `.uploads`) is **created on demand**
///   (`create_dir_all`) rather than required to pre-exist, so the very first
///   chat attachment lands without a separate `mkdir` round-trip.
/// * On a name collision the new file is given a ` (1)`, ` (2)`… suffix
///   (extension-preserving) instead of clobbering the existing file, so two
///   attachments of the same basename coexist.
///
/// The destination is confined to the agent realm: `name` must be a bare
/// component and the resolved directory (after creation) must canonicalize
/// inside the realm (no `..`/symlink/absolute escape). Returns
/// `{ path, name, size }` — the realm-relative path of the **stored** file, its
/// final (possibly suffixed) name, and the byte count — everything the composer
/// needs to compose the `file-upload` message block.
pub fn fs_upload_unique(state: &Mutex<Backend>, agent_id: &str, query: &str, body: &[u8]) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    let relative_dir = extract_param(query, "path").unwrap_or_default();
    let name = match extract_param(query, "name") {
        Some(n) if !n.is_empty() => n,
        _ => return HttpReply::error(400, "missing name parameter"),
    };
    if !is_bare_name(&name) {
        return HttpReply::error(400, "invalid file name");
    }
    // A `..` component in the directory would let the upload escape the realm
    // before confinement (which only runs after the dir is created).
    if relative_dir.split('/').any(|seg| seg == "..") || relative_dir.starts_with('/') {
        return HttpReply::error(403, "path outside agent realm");
    }

    // Resolve the realm root, then create the destination directory under it on
    // demand so a first-ever chat attachment doesn't need a prior mkdir.
    let Some(root) = confined_path(&folder, "") else {
        return HttpReply::error(403, "realm root unresolved");
    };
    let dir = root.join(&relative_dir);
    if std::fs::create_dir_all(&dir).is_err() {
        return HttpReply::error(502, "could not create destination directory");
    }
    // Re-confine now that the directory exists — defends against a symlinked
    // component pointing outside the realm.
    let dir = match confined_path(&folder, &relative_dir) {
        Some(p) => p,
        None => return HttpReply::error(403, "path outside agent realm"),
    };

    let final_name = unique_name(&dir, &name);
    let dest = dir.join(&final_name);
    if std::fs::write(&dest, body).is_err() {
        return HttpReply::error(502, "write failed");
    }

    HttpReply::ok(&serde_json::json!({
        "path": rel_child(&relative_dir, &final_name),
        "name": final_name,
        "size": body.len(),
    }))
}

/// Collision-free filename inside `dir`: returns `name` unchanged if free, else
/// the first ` (n)` variant (extension-preserving) that does not yet exist —
/// `report.pdf` → `report (1).pdf`, `notes` → `notes (1)`. The probe is bounded
/// so a pathological directory can't loop forever; the final fallback embeds a
/// nanosecond suffix that is effectively unique.
fn unique_name(dir: &std::path::Path, name: &str) -> String {
    if !dir.join(name).exists() {
        return name.to_owned();
    }
    // Split into stem + extension so the suffix lands before the dot.
    let (stem, ext) = match name.rsplit_once('.') {
        // Leading-dot dotfiles (".env") have an empty stem — treat the whole
        // thing as the stem so we don't produce " (1).env" from nothing.
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    for n in 1..10_000 {
        let candidate = format!("{stem} ({n}){ext}");
        if !dir.join(&candidate).exists() {
            return candidate;
        }
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    format!("{stem} ({stamp}){ext}")
}
