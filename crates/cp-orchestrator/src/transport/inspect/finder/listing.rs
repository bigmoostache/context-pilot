//! Read views for the Finder: directory listing, file preview, conversation.

use std::path::PathBuf;
use std::sync::Mutex;

use crate::transport::rest::HttpReply;
use crate::transport::Backend;

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
    let path = PathBuf::from(&folder)
        .join(".context-pilot")
        .join("shared")
        .join("tree-descriptions.yaml");

    let Ok(raw) = std::fs::read(&path) else {
        return HttpReply::ok(&serde_json::json!({}));
    };
    let Ok(doc): Result<serde_yaml::Value, _> = serde_yaml::from_slice(&raw) else {
        return HttpReply::ok(&serde_json::json!({}));
    };

    let mut map = serde_json::Map::new();
    if let Some(entries) = doc.as_mapping() {
        for value in entries.values() {
            let path = value.get("path").and_then(serde_yaml::Value::as_str);
            let desc = value.get("description").and_then(serde_yaml::Value::as_str);
            if let (Some(p), Some(d)) = (path, desc) {
                if !p.is_empty() && !d.is_empty() {
                    let _prev = map.insert(p.to_owned(), serde_json::Value::String(d.to_owned()));
                }
            }
        }
    }

    HttpReply::ok(&serde_json::Value::Object(map))
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
