//! **Finder** endpoints — confined directory listing, file preview, and
//! conversation message retrieval for an agent's working directory.
//!
//! * [`fs_list`] — `GET /api/agent/{id}/fs?path=` — directory listing confined
//!   to the agent's folder (no `..` escape, no symlink escape).
//! * [`fs_preview`] — `GET /api/agent/{id}/fs/preview?path=` — file content
//!   preview (text, capped).
//! * [`conversation`] — `GET /api/agent/{id}/conversation` — conversation
//!   messages from the agent's `messages/` directory.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::transport::rest::HttpReply;
use crate::transport::Backend;

/// Maximum file size for downloads (10 MiB).
const MAX_DOWNLOAD_BYTES: u64 = 10 * 1024 * 1024;

/// Maximum zip archive size for folder downloads (100 MiB).
const MAX_ZIP_BYTES: u64 = 100 * 1024 * 1024;

/// `GET /api/agent/{id}/fs/download?path=` — file or folder download.
///
/// **Files** are returned as raw bytes with `Content-Disposition: attachment`,
/// capped at [`MAX_DOWNLOAD_BYTES`].
///
/// **Folders** are zipped into a temporary archive (`/tmp`), returned as
/// `{dirname}.zip`, capped at [`MAX_ZIP_BYTES`], then cleaned up. Uses the
/// system `zip` command (available on macOS and most Linux distros).
///
/// Returns `Ok((bytes, filename))` on success, `Err(HttpReply)` on error.
pub fn fs_download(
    state: &Mutex<Backend>,
    agent_id: &str,
    query: &str,
) -> Result<(Vec<u8>, String), HttpReply> {
    let folder = agent_folder(state, agent_id)?;
    let relative = match extract_param(query, "path") {
        Some(p) if !p.is_empty() => p,
        _ => return Err(HttpReply::error(400, "missing path parameter")),
    };
    let target = match confined_path(&folder, &relative) {
        Some(p) => p,
        None => return Err(HttpReply::error(403, "path outside agent realm")),
    };

    if target.is_dir() {
        return zip_and_download(&target);
    }

    if !target.is_file() {
        return Err(HttpReply::error(404, "not a file or directory"));
    }

    let meta = std::fs::metadata(&target).map_err(|_| HttpReply::error(404, "file not found"))?;
    if meta.len() > MAX_DOWNLOAD_BYTES {
        return Err(HttpReply::error(413, "file too large for download"));
    }

    let bytes = std::fs::read(&target).map_err(|_| HttpReply::error(502, "read failed"))?;
    let filename = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download")
        .to_owned();

    Ok((bytes, filename))
}

/// Zip a directory into `/tmp` and return its bytes + filename.
fn zip_and_download(dir: &Path) -> Result<(Vec<u8>, String), HttpReply> {
    let dirname = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("folder");

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let tmp = format!("/tmp/cp-dl-{}-{now}.zip", std::process::id());

    let output = std::process::Command::new("zip")
        .args(["-r", "-q", &tmp, "."])
        .current_dir(dir)
        .output()
        .map_err(|_| HttpReply::error(502, "zip command not available"))?;

    if !output.status.success() {
        drop(std::fs::remove_file(&tmp));
        return Err(HttpReply::error(502, "zip failed"));
    }

    let meta = std::fs::metadata(&tmp).map_err(|_| {
        drop(std::fs::remove_file(&tmp));
        HttpReply::error(502, "zip read failed")
    })?;
    if meta.len() > MAX_ZIP_BYTES {
        drop(std::fs::remove_file(&tmp));
        return Err(HttpReply::error(413, "zipped folder too large"));
    }

    let bytes = std::fs::read(&tmp).map_err(|_| {
        drop(std::fs::remove_file(&tmp));
        HttpReply::error(502, "zip read failed")
    })?;
    drop(std::fs::remove_file(&tmp));

    let zip_name = format!("{dirname}.zip");
    Ok((bytes, zip_name))
}

/// Maximum file size returned by the preview endpoint (256 KiB).
const MAX_PREVIEW_BYTES: u64 = 256 * 1024;

/// Maximum number of conversation messages returned per request.
const MAX_CONVERSATION_MESSAGES: usize = 200;

/// `GET /api/agent/{id}/fs?path=` — confined directory listing.
///
/// Lists one level of the agent's working directory at the given relative
/// path. Returns an array of `FinderNode` objects. The path is confined to
/// the agent's folder — any attempt to escape (via `..`, symlinks, or
/// absolute paths) is rejected with a `403`.
pub fn fs_list(state: &Mutex<Backend>, agent_id: &str, query: &str) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    let relative = extract_param(query, "path").unwrap_or_default();
    let target = match confined_path(&folder, &relative) {
        Some(p) => p,
        None => return HttpReply::error(403, "path outside agent realm"),
    };

    let entries = match std::fs::read_dir(&target) {
        Ok(rd) => rd,
        Err(_) => return HttpReply::error(404, "directory not found"),
    };

    let mut nodes: Vec<serde_json::Value> = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let Ok(meta) = entry.metadata() else { continue };
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else { continue };

        // Skip hidden files/dirs (starting with .)
        if name_str.starts_with('.') {
            continue;
        }

        let entry_path = if relative.is_empty() {
            name_str.to_owned()
        } else {
            format!("{relative}/{name_str}")
        };

        let kind = if meta.is_dir() {
            "folder"
        } else {
            infer_kind(name_str)
        };

        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as u64);

        let mut node = serde_json::json!({
            "name": name_str,
            "path": entry_path,
            "kind": kind,
            "modified": modified_ms,
        });

        if meta.is_file() {
            let _prev = node
                .as_object_mut()
                .expect("just built")
                .insert("size".to_owned(), serde_json::json!(meta.len()));
        } else if meta.is_dir() {
            // Direct (non-hidden) child count, so every view can render
            // "N items" on a folder without a second round-trip. Mirrors the
            // hidden-file skip below, so the count matches what a listing of
            // this folder would actually show.
            let count = count_visible_children(&entry.path());
            let _prev = node
                .as_object_mut()
                .expect("just built")
                .insert("count".to_owned(), serde_json::json!(count));
        }

        nodes.push(node);
    }

    // Sort: folders first, then alphabetically by name.
    nodes.sort_by(|a, b| {
        let a_folder = a.get("kind").and_then(serde_json::Value::as_str) == Some("folder");
        let b_folder = b.get("kind").and_then(serde_json::Value::as_str) == Some("folder");
        b_folder
            .cmp(&a_folder)
            .then_with(|| {
                let a_name = a.get("name").and_then(serde_json::Value::as_str).unwrap_or("");
                let b_name = b.get("name").and_then(serde_json::Value::as_str).unwrap_or("");
                a_name.to_lowercase().cmp(&b_name.to_lowercase())
            })
    });

    HttpReply::ok(&nodes)
}

/// `GET /api/agent/{id}/fs/preview?path=` — file content preview.
///
/// Returns the first [`MAX_PREVIEW_BYTES`] of a file as a JSON object with
/// `content` (text) and `truncated` (bool). Binary-looking files are rejected
/// with a 415.
pub fn fs_preview(state: &Mutex<Backend>, agent_id: &str, query: &str) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    let relative = match extract_param(query, "path") {
        Some(p) if !p.is_empty() => p,
        _ => return HttpReply::error(400, "missing path parameter"),
    };
    let target = match confined_path(&folder, &relative) {
        Some(p) => p,
        None => return HttpReply::error(403, "path outside agent realm"),
    };
    if !target.is_file() {
        return HttpReply::error(404, "file not found");
    }

    let meta = match std::fs::metadata(&target) {
        Ok(m) => m,
        Err(_) => return HttpReply::error(404, "file not found"),
    };
    let file_size = meta.len();
    let truncated = file_size > MAX_PREVIEW_BYTES;
    let read_size = file_size.min(MAX_PREVIEW_BYTES) as usize;

    let bytes = match std::fs::read(&target) {
        Ok(b) => b,
        Err(_) => return HttpReply::error(502, "read failed"),
    };
    let slice = bytes.get(..read_size).unwrap_or(&bytes);

    // Reject binary content (check for null bytes in first 8KB).
    let check_len = slice.len().min(8192);
    if let Some(sample) = slice.get(..check_len) {
        if sample.iter().any(|&b| b == 0) {
            return HttpReply::error(415, "binary file");
        }
    }

    let content = String::from_utf8_lossy(slice);
    HttpReply::ok(&serde_json::json!({
        "content": content,
        "size": file_size,
        "truncated": truncated,
    }))
}

/// `GET /api/agent/{id}/conversation` — conversation messages.
///
/// Reads YAML message files from the agent's `.context-pilot/messages/`
/// directory, sorted by filename (which encodes chronological order), capped
/// at [`MAX_CONVERSATION_MESSAGES`] most recent.
pub fn conversation(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    let messages_dir = PathBuf::from(&folder)
        .join(".context-pilot")
        .join("messages");

    let entries = match std::fs::read_dir(&messages_dir) {
        Ok(rd) => rd,
        Err(_) => return HttpReply::ok(&serde_json::json!([])),
    };

    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(std::ffi::OsStr::to_str)
                .map_or(false, |ext| ext == "yaml" || ext == "yml")
        })
        .collect();

    // Sort by filename (UID_*.yaml encodes insertion order).
    files.sort();

    // Keep only the most recent N.
    if files.len() > MAX_CONVERSATION_MESSAGES {
        let skip = files.len().saturating_sub(MAX_CONVERSATION_MESSAGES);
        files = files.split_off(skip);
    }

    let mut messages: Vec<serde_json::Value> = Vec::new();
    for path in &files {
        let Ok(raw) = std::fs::read(path) else { continue };
        let Ok(val): Result<serde_json::Value, _> = serde_yaml::from_slice(&raw) else {
            continue;
        };
        messages.push(val);
    }

    HttpReply::ok(&messages)
}

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
    if name.contains('/') || name.contains('\\') || name.contains('\0') || name == "." || name == ".."
    {
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

    let rel_path = if relative_dir.is_empty() {
        name.clone()
    } else {
        format!("{relative_dir}/{name}")
    };
    HttpReply::ok(&serde_json::json!({
        "written": body.len(),
        "path": rel_path,
    }))
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

// ── Helpers ────────────────────────────────────────────────────────────

/// Resolve the agent's working directory from the registry record.
fn agent_folder(state: &Mutex<Backend>, agent_id: &str) -> Result<String, HttpReply> {
    let entry = crate::transport::rest::resolve_entry(state, agent_id)?;
    Ok(entry.folder)
}

/// Canonicalize and confine a relative path to a root directory.
///
/// Returns `None` if the resolved path escapes `root` (via `..`, symlinks,
/// or absolute paths). An empty `relative` resolves to `root` itself.
fn confined_path(root: &str, relative: &str) -> Option<PathBuf> {
    let root_path = Path::new(root);
    let root_canonical = root_path.canonicalize().ok()?;

    if relative.is_empty() || relative == "." {
        return Some(root_canonical);
    }

    // Reject absolute paths outright.
    if relative.starts_with('/') {
        return None;
    }

    let candidate = root_path.join(relative);
    let canonical = candidate.canonicalize().ok()?;
    if canonical.starts_with(&root_canonical) {
        Some(canonical)
    } else {
        None
    }
}

/// Count the non-hidden direct children of a directory.
///
/// Used to annotate folder nodes with an item count for the Finder views.
/// Returns `0` on any I/O error (unreadable dir) — a best-effort hint, never
/// an error surface. Skips dotfiles to match [`fs_list`]'s own filtering, so
/// the count equals what opening the folder would display.
fn count_visible_children(dir: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    rd.filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| !n.starts_with('.'))
        })
        .count()
}

/// Infer a `FinderKind` string from a filename's extension.
fn infer_kind(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "go" | "c" | "cpp" | "h" | "hpp" | "java"
        | "rb" | "sh" | "bash" | "zsh" | "lua" | "zig" | "swift" | "kt" | "scala" | "ex"
        | "exs" | "erl" | "hs" | "ml" | "css" | "scss" | "html" | "sql" | "r" | "pl"
        | "php" | "cs" | "fs" | "vue" | "svelte" | "dart" | "nim" | "v" | "wasm" => "code",
        "md" | "mdx" => "markdown",
        "json" | "jsonl" | "json5" => "json",
        "pdf" => "pdf",
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "bmp" | "ico" | "tiff" | "heic" => {
            "image"
        }
        "csv" | "xlsx" | "xls" | "ods" | "tsv" => "sheet",
        "pptx" | "ppt" | "odp" => "slides",
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "zst" => "archive",
        "mp3" | "wav" | "flac" | "m4a" | "ogg" | "aac" | "wma" => "audio",
        "mp4" | "mov" | "avi" | "mkv" | "webm" | "wmv" | "flv" => "video",
        "txt" | "log" | "yml" | "yaml" | "toml" | "cfg" | "ini" | "env" | "conf"
        | "properties" | "lock" | "editorconfig" | "gitignore" | "dockerignore" => "doc",
        _ => "binary",
    }
}

/// Extract a query parameter value by key from a raw query string.
fn extract_param(query: &str, key: &str) -> Option<String> {
    query
        .split('&')
        .filter(|s| !s.is_empty())
        .find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            if k == key {
                // Percent-decode the value (basic: %20 → space, %2F → /).
                Some(percent_decode(v))
            } else {
                None
            }
        })
}

/// Basic percent-decoding for path parameters.
fn percent_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            } else {
                result.push('%');
                result.push_str(&hex);
            }
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confined_path_rejects_escape() {
        // Trying to escape via ..
        assert!(confined_path("/tmp", "../etc/passwd").is_none());
        // Absolute path rejected
        assert!(confined_path("/tmp", "/etc/passwd").is_none());
    }

    #[test]
    fn confined_path_accepts_valid() {
        // Empty relative = root itself
        let root = std::env::temp_dir();
        let root_str = root.to_string_lossy().to_string();
        let result = confined_path(&root_str, "");
        assert!(result.is_some());
    }

    #[test]
    fn infer_kind_classifies_extensions() {
        assert_eq!(infer_kind("main.rs"), "code");
        assert_eq!(infer_kind("README.md"), "markdown");
        assert_eq!(infer_kind("data.json"), "json");
        assert_eq!(infer_kind("photo.png"), "image");
        assert_eq!(infer_kind("config.yaml"), "doc");
        assert_eq!(infer_kind("archive.zip"), "archive");
        assert_eq!(infer_kind("mystery"), "binary");
    }

    #[test]
    fn percent_decode_handles_common_cases() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("src%2Fmain.rs"), "src/main.rs");
        assert_eq!(percent_decode("plain"), "plain");
    }

    #[test]
    fn extract_param_finds_value() {
        assert_eq!(
            extract_param("path=src%2Flib&format=json", "path"),
            Some("src/lib".to_owned())
        );
        assert_eq!(
            extract_param("path=src%2Flib&format=json", "format"),
            Some("json".to_owned())
        );
        assert_eq!(extract_param("path=src", "missing"), None);
    }
}
