//! Read views for the Finder: directory listing, file preview, conversation.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::transport::Backend;
use crate::transport::rest::HttpReply;

use super::support::{agent_folder, confined_path, count_visible_children, extract_param, infer_kind};

/// `GET /api/agent/{id}/fs/descriptions` — the agent's tree descriptions.
///
/// Reads the agent's `tree` module persistence
/// (`<folder>/.context-pilot/shared/tree-descriptions.yaml`) and returns a flat
/// JSON object mapping each described **realm-relative path** to its description
/// text — exactly the keys the Finder lists, so a node can show an info badge
/// when (and only when) the agent has written a description for it.
///
/// The on-disk file is a [`YamlSync`](cp-base) map keyed by an opaque per-entry
/// hash, each value carrying `{ path, description, last_edited_ms }`; this
/// flattens it to `{ path: description }`. A missing or unparseable file yields
/// an empty object (a realm with no descriptions is the normal case, never an
/// error). The agent id is still resolved so an unknown agent is a `404`.
pub fn fs_descriptions(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    let path = PathBuf::from(&folder).join(".context-pilot").join("shared").join("tree-descriptions.yaml");

    let Ok(raw) = std::fs::read(&path) else {
        return HttpReply::ok(&serde_json::json!({}));
    };
    let Ok(doc): Result<serde_yaml::Value, _> = serde_yaml::from_slice(&raw) else {
        return HttpReply::ok(&serde_json::json!({}));
    };

    let mut map = serde_json::Map::new();
    if let Some(entries) = doc.as_mapping() {
        for value in entries.values() {
            let rel = value.get("path").and_then(serde_yaml::Value::as_str);
            let desc = value.get("description").and_then(serde_yaml::Value::as_str);
            if let (Some(p), Some(d)) = (rel, desc)
                && !p.is_empty()
                && !d.is_empty()
            {
                let _prev = map.insert(p.to_owned(), serde_json::Value::String(d.to_owned()));
            }
        }
    }

    HttpReply::ok(&serde_json::Value::Object(map))
}

/// Maximum file size returned by the preview endpoint (1 MiB).
const MAX_PREVIEW_BYTES: u64 = 1024 * 1024;

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
    let Some(target) = confined_path(&folder, &relative) else {
        return HttpReply::error(403, "path outside agent realm");
    };

    let Ok(entries) = std::fs::read_dir(&target) else {
        return HttpReply::error(404, "directory not found");
    };

    let mut nodes: Vec<serde_json::Value> = Vec::new();
    for raw in entries {
        let Ok(entry) = raw else { continue };
        let Ok(meta) = entry.metadata() else { continue };
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else { continue };

        // Skip hidden files/dirs (starting with .)
        if name_str.starts_with('.') {
            continue;
        }

        let entry_path = if relative.is_empty() { name_str.to_owned() } else { format!("{relative}/{name_str}") };

        nodes.push(fs_node(name_str, &entry_path, &entry.path(), &meta));
    }

    // Sort: folders first, then alphabetically by name.
    nodes.sort_by(|a, b| {
        let a_folder = a.get("kind").and_then(serde_json::Value::as_str) == Some("folder");
        let b_folder = b.get("kind").and_then(serde_json::Value::as_str) == Some("folder");
        b_folder.cmp(&a_folder).then_with(|| {
            let a_name = a.get("name").and_then(serde_json::Value::as_str).unwrap_or("");
            let b_name = b.get("name").and_then(serde_json::Value::as_str).unwrap_or("");
            a_name.to_lowercase().cmp(&b_name.to_lowercase())
        })
    });

    HttpReply::ok(&nodes)
}

/// Build one Finder `FinderNode` JSON object for a directory entry.
///
/// Owns the `serde_json::Map` directly (never `as_object_mut().expect(...)`),
/// so there is no panic path. File entries carry a `size`; directory entries
/// carry a `count` of their non-hidden direct children (so a view can render
/// "N items" without a second round-trip). The two branches are mutually
/// exclusive, so they are two independent `if`s rather than an `else if` chain
/// (which would demand an unused final `else`).
fn fs_node(name: &str, path: &str, entry_path: &Path, meta: &std::fs::Metadata) -> serde_json::Value {
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    let kind = if meta.is_dir() { "folder" } else { infer_kind(name) };

    let mut map = serde_json::Map::new();
    drop(map.insert("name".to_owned(), serde_json::json!(name)));
    drop(map.insert("path".to_owned(), serde_json::json!(path)));
    drop(map.insert("kind".to_owned(), serde_json::json!(kind)));
    drop(map.insert("modified".to_owned(), serde_json::json!(modified_ms)));
    if meta.is_file() {
        drop(map.insert("size".to_owned(), serde_json::json!(meta.len())));
    }
    if meta.is_dir() {
        drop(map.insert("count".to_owned(), serde_json::json!(count_visible_children(entry_path))));
    }
    serde_json::Value::Object(map)
}
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
    let Some(target) = confined_path(&folder, &relative) else {
        return HttpReply::error(403, "path outside agent realm");
    };
    if !target.is_file() {
        return HttpReply::error(404, "file not found");
    }

    let Ok(meta) = std::fs::metadata(&target) else {
        return HttpReply::error(404, "file not found");
    };
    let file_size = meta.len();
    let truncated = file_size > MAX_PREVIEW_BYTES;
    let read_size = usize::try_from(file_size.min(MAX_PREVIEW_BYTES)).unwrap_or(usize::MAX);

    let Ok(bytes) = std::fs::read(&target) else {
        return HttpReply::error(502, "read failed");
    };
    let slice = bytes.get(..read_size).unwrap_or(&bytes);

    // Reject binary content (check for null bytes in first 8KB).
    let check_len = slice.len().min(8192);
    if let Some(sample) = slice.get(..check_len)
        && sample.contains(&0)
    {
        return HttpReply::error(415, "binary file");
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
    let messages_dir = PathBuf::from(&folder).join(".context-pilot").join("messages");

    let Ok(entries) = std::fs::read_dir(&messages_dir) else {
        return HttpReply::ok(&serde_json::json!([]));
    };

    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(std::ffi::OsStr::to_str).is_some_and(|ext| ext == "yaml" || ext == "yml"))
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
