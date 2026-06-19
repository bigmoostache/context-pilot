//! Write operations for the Finder: upload, mkdir, rename, move. Every write
//! is confined to the agent realm and never clobbers an existing entry.

use std::sync::Mutex;

use crate::transport::rest::HttpReply;
use crate::transport::Backend;

use super::support::{agent_folder, confined_path, extract_param};

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

/// `POST /api/agent/{id}/fs/mkdir?path={dir}&name={name}` — create a folder.
///
/// Creates a single new directory `name` inside the confined directory `path`
/// (empty = realm root). Powers the Finder's "New Folder" action (toolbar +
/// empty-space context menu). Takes no body.
///
/// The parent directory is confined to the agent realm (no `..`/symlink/absolute
/// escape) and must already exist; `name` must be a bare component (no path
/// separators, not `.`/`..`) so the new folder can never land outside the realm.
/// An already-existing entry with that name is a `409` (never silently reused).
/// Returns `{ created }` — the realm-relative path of the new folder.
pub fn fs_mkdir(state: &Mutex<Backend>, agent_id: &str, query: &str) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    let relative_dir = extract_param(query, "path").unwrap_or_default();
    let name = match extract_param(query, "name") {
        Some(n) if !n.is_empty() => n,
        _ => return HttpReply::error(400, "missing name parameter"),
    };

    // The new folder's name must be a bare component — a separator or `..` would
    // let it escape the confined parent directory.
    if !is_bare_name(&name) {
        return HttpReply::error(400, "invalid folder name");
    }

    let dir = match confined_path(&folder, &relative_dir) {
        Some(p) => p,
        None => return HttpReply::error(403, "path outside agent realm"),
    };
    if !dir.is_dir() {
        return HttpReply::error(404, "parent directory not found");
    }

    let dest = dir.join(&name);
    if dest.exists() {
        return HttpReply::error(409, "an entry with that name already exists");
    }
    if std::fs::create_dir(&dest).is_err() {
        return HttpReply::error(502, "create failed");
    }

    HttpReply::ok(&serde_json::json!({ "created": rel_child(&relative_dir, &name) }))
}

/// `POST /api/agent/{id}/fs/rename?path={item}&name={newname}` — rename one entry.
///
/// Renames the file or folder at the confined realm-relative `path` to the bare
/// new name `name`, keeping it in the same parent directory. Powers the Finder's
/// inline rename (double-click the name / Enter / context-menu Rename).
///
/// The source is confined to the agent realm (no `..`/symlink/absolute escape)
/// and must exist; `name` must be a bare component (no path separators, not
/// `.`/`..`) so the rename can never relocate the entry outside its parent.
/// A rename to the unchanged name is a no-op success; a name already taken by a
/// different entry is a `409` (never clobbers). Returns `{ renamed }` — the new
/// realm-relative path.
pub fn fs_rename(state: &Mutex<Backend>, agent_id: &str, query: &str) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    let relative = match extract_param(query, "path") {
        Some(p) if !p.is_empty() => p,
        _ => return HttpReply::error(400, "missing path parameter"),
    };
    let name = match extract_param(query, "name") {
        Some(n) if !n.is_empty() => n,
        _ => return HttpReply::error(400, "missing name parameter"),
    };

    // The new name must be a bare component — a separator or `..` would let the
    // rename relocate the entry outside its parent directory.
    if !is_bare_name(&name) {
        return HttpReply::error(400, "invalid name");
    }

    let src = match confined_path(&folder, &relative) {
        Some(p) => p,
        None => return HttpReply::error(403, "path outside agent realm"),
    };
    if !src.exists() {
        return HttpReply::error(404, "entry not found");
    }
    let Some(parent) = src.parent() else {
        return HttpReply::error(400, "cannot rename the realm root");
    };

    let dest = parent.join(&name);
    // A rename to the same name is a harmless no-op (the inline editor commits on
    // blur even when the text is unchanged).
    if dest == src {
        return HttpReply::ok(&serde_json::json!({ "renamed": rel_sibling(&relative, &name) }));
    }
    if dest.exists() {
        return HttpReply::error(409, "an entry with that name already exists");
    }
    if std::fs::rename(&src, &dest).is_err() {
        return HttpReply::error(502, "rename failed");
    }

    HttpReply::ok(&serde_json::json!({ "renamed": rel_sibling(&relative, &name) }))
}

/// `POST /api/agent/{id}/fs/move` — move one or more entries into a directory.
///
/// Body is JSON `{ "items": ["rel/a", "rel/b"], "dest": "rel/dir" }`: each
/// `items` entry is a realm-relative file/folder path, `dest` the realm-relative
/// destination directory (empty = realm root). Powers the Finder's internal
/// drag-and-drop (drop a selection onto a folder = move it inside).
///
/// Every source and the destination directory are confined to the agent realm
/// (no `..`/symlink/absolute escape). Each item is moved as
/// `dest/<basename(item)>` via [`std::fs::rename`]. Guards, per item:
/// * **already there** (`dest` is the item's current parent) → skipped, counted
///   as a no-op success so a stray self-drop is harmless.
/// * **destination occupied** → `409`, never clobbers an existing entry.
/// * **into-own-descendant** (moving a folder inside itself) → `409`.
///
/// Returns `{ moved, skipped }` (entries actually renamed vs. no-op'd). A single
/// failing item aborts with the matching error status (best-effort partial moves
/// already applied are not rolled back — the listing refresh shows the truth).
pub fn fs_move(state: &Mutex<Backend>, agent_id: &str, body: &[u8]) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };

    let parsed: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return HttpReply::error(400, "malformed move request"),
    };
    let items: Vec<String> = parsed
        .get("items")
        .and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()
        })
        .unwrap_or_default();
    if items.is_empty() {
        return HttpReply::error(400, "no items to move");
    }
    let dest_rel = parsed.get("dest").and_then(serde_json::Value::as_str).unwrap_or("");

    let dest_dir = match confined_path(&folder, dest_rel) {
        Some(p) => p,
        None => return HttpReply::error(403, "destination outside agent realm"),
    };
    if !dest_dir.is_dir() {
        return HttpReply::error(404, "destination directory not found");
    }

    let mut moved = 0_usize;
    let mut skipped = 0_usize;
    for item in &items {
        let src = match confined_path(&folder, item) {
            Some(p) => p,
            None => return HttpReply::error(403, "source outside agent realm"),
        };
        let Some(base) = src.file_name() else {
            return HttpReply::error(400, "invalid source path");
        };
        let dest_path = dest_dir.join(base);

        // Already in the destination directory → nothing to do.
        if dest_path == src {
            skipped += 1;
            continue;
        }
        // Refuse to move a directory inside itself or one of its descendants.
        if src.is_dir() && dest_dir.starts_with(&src) {
            return HttpReply::error(409, "cannot move a folder into itself");
        }
        // Never clobber an existing entry.
        if dest_path.exists() {
            return HttpReply::error(409, "an entry with that name already exists");
        }
        if std::fs::rename(&src, &dest_path).is_err() {
            return HttpReply::error(502, "move failed");
        }
        moved += 1;
    }

    HttpReply::ok(&serde_json::json!({ "moved": moved, "skipped": skipped }))
}

// ── Shared write helpers ────────────────────────────────────────────────

/// True when `name` is a safe bare path component — no separators, no `\0`, and
/// not the `.`/`..` traversal aliases — so a write built from it can never
/// escape its parent directory.
fn is_bare_name(name: &str) -> bool {
    !(name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
        || name == "."
        || name == "..")
}

/// Realm-relative path of a new `name` created inside directory `dir`
/// (empty `dir` = realm root).
fn rel_child(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

/// Realm-relative path of an entry renamed to `name`, given the original
/// entry's realm-relative path (replaces the last segment).
fn rel_sibling(original_rel: &str, name: &str) -> String {
    match original_rel.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/{name}"),
        None => name.to_owned(),
    }
}
