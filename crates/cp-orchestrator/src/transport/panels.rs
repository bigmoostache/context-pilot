//! Cockpit **panel inspection** endpoints — read agent state files and reshape
//! to maquette-compatible JSON.
//!
//! Each handler reads from the agent's tier-② persistence files (as specified
//! in the design doc v7 §3.1) and returns structured JSON. Global module data
//! comes from `config.json`, per-worker data from `states/<worker>.json`, and
//! shared files from the `shared/` directory.
//!
//! Worker-scoped endpoints accept an optional `?worker=<id>` query parameter;
//! when absent they fall back to the first worker found.

use std::path::Path;
use std::sync::Mutex;

use super::rest::HttpReply;
use super::Backend;

/// `GET /api/agent/{id}/panels` — list of inspectable cockpit panel types.
///
/// Returns a fixed catalogue of panel kinds the frontend can query, not the
/// runtime context list (which is ephemeral). Each entry names the endpoint
/// path the frontend should hit for that panel's data.
pub fn panel_list(_state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let panels = serde_json::json!([
        { "id": "memory",     "name": "Memories",   "kind": "memory",     "endpoint": format!("/api/agent/{agent_id}/memory") },
        { "id": "todos",      "name": "Todos",      "kind": "todo",       "endpoint": format!("/api/agent/{agent_id}/todos") },
        { "id": "spine",      "name": "Spine",      "kind": "spine",      "endpoint": format!("/api/agent/{agent_id}/spine") },
        { "id": "queue",      "name": "Queue",      "kind": "queue",      "endpoint": format!("/api/agent/{agent_id}/queue") },
        { "id": "scratchpad", "name": "Scratchpad",  "kind": "scratchpad", "endpoint": format!("/api/agent/{agent_id}/scratchpad") },
        { "id": "tree",       "name": "Tree",       "kind": "tree",       "endpoint": format!("/api/agent/{agent_id}/tree") },
        { "id": "callbacks",  "name": "Callbacks",  "kind": "callback",   "endpoint": format!("/api/agent/{agent_id}/callbacks") },
        { "id": "threads",    "name": "Threads",    "kind": "threads",    "endpoint": format!("/api/agent/{agent_id}/threads") },
    ]);
    HttpReply::ok(&panels)
}

/// `GET /api/agent/{id}/memory` — memory items from `shared/memories.yaml`.
pub fn memory(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    read_shared_yaml(state, &folder, "memories.yaml")
}

/// `GET /api/agent/{id}/todos` — todo items from the worker's module state.
pub fn todos(state: &Mutex<Backend>, agent_id: &str, query: &str) -> HttpReply {
    worker_module(state, agent_id, query, "todo")
}

/// `GET /api/agent/{id}/spine` — spine notifications + config.
pub fn spine(state: &Mutex<Backend>, agent_id: &str, query: &str) -> HttpReply {
    worker_module(state, agent_id, query, "spine")
}

/// `GET /api/agent/{id}/queue` — queue state (active, queued calls).
pub fn queue(state: &Mutex<Backend>, agent_id: &str, query: &str) -> HttpReply {
    worker_module(state, agent_id, query, "queue")
}

/// `GET /api/agent/{id}/scratchpad` — scratchpad cells.
pub fn scratchpad(state: &Mutex<Backend>, agent_id: &str, query: &str) -> HttpReply {
    worker_module(state, agent_id, query, "scratchpad")
}

/// `GET /api/agent/{id}/tree` — tree descriptions from shared YAML.
pub fn tree(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    read_shared_yaml(state, &folder, "tree-descriptions.yaml")
}

/// `GET /api/agent/{id}/callbacks` — callback definitions from shared YAML.
pub fn callbacks(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    read_shared_yaml(state, &folder, "callbacks.yaml")
}

// ── Helpers ────────────────────────────────────────────────────────────

/// Resolve the agent's working directory from the registry record.
fn agent_folder(state: &Mutex<Backend>, agent_id: &str) -> Result<String, HttpReply> {
    let entry = super::rest::resolve_entry(state, agent_id)?;
    Ok(entry.folder)
}

/// Read a per-worker module's persisted state from `states/<worker>.json`.
///
/// Extracts `modules.<module_key>` from the worker state. Falls back to the
/// first discovered worker when no `?worker=` query parameter is given.
fn worker_module(
    state: &Mutex<Backend>,
    agent_id: &str,
    query: &str,
    module_key: &str,
) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    let folder_path = Path::new(&folder);

    let worker_id = extract_worker_param(query);

    let Ok(mut backend) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };

    let wid = match worker_id {
        Some(id) => id,
        None => {
            // Fall back to the first worker found.
            let workers = match backend.inspect_mut().list_workers(folder_path) {
                Ok(w) => w,
                Err(_) => return HttpReply::error(404, "cannot list workers"),
            };
            match workers.first() {
                Some(w) => w.clone(),
                None => return HttpReply::error(404, "no workers found"),
            }
        }
    };

    let worker_state = backend.inspect_mut().read_worker(folder_path, &wid);
    match worker_state {
        Ok(ws) => {
            let module_data = ws
                .get("modules")
                .and_then(|m| m.get(module_key))
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            HttpReply::ok(&module_data)
        }
        Err(_) => HttpReply::error(404, "worker state unavailable"),
    }
}

/// Read a shared YAML file, parse it, and return as JSON.
fn read_shared_yaml(state: &Mutex<Backend>, folder: &str, filename: &str) -> HttpReply {
    let folder_path = Path::new(folder);
    let Ok(mut backend) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let bytes = backend.inspect_mut().read_shared(folder_path, filename);
    match bytes {
        Ok(raw) => {
            // Parse YAML → serde_json::Value for uniform JSON responses.
            let yaml_val: Result<serde_json::Value, _> = serde_yaml::from_slice(&raw);
            match yaml_val {
                Ok(val) => HttpReply::ok(&val),
                Err(_) => HttpReply::error(502, "YAML parse failed"),
            }
        }
        Err(_) => HttpReply::error(404, "shared file not found"),
    }
}

/// Extract the `worker` query parameter from a raw query string.
fn extract_worker_param(query: &str) -> Option<String> {
    query
        .split('&')
        .filter(|s| !s.is_empty())
        .find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            if k == "worker" { Some(v.to_owned()) } else { None }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_worker_param_finds_value() {
        assert_eq!(extract_worker_param("worker=abc123"), Some("abc123".to_owned()));
        assert_eq!(extract_worker_param("agent=x&worker=def"), Some("def".to_owned()));
        assert_eq!(extract_worker_param("agent=x"), None);
        assert_eq!(extract_worker_param(""), None);
    }
}
