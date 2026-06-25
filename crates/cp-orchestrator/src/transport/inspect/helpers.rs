//! Shared helpers for inspection-panel endpoints.

use std::path::Path;
use std::sync::Mutex;

use crate::transport::Backend;
use crate::transport::rest::HttpReply;

/// Resolve the agent's working directory from the registry record.
pub(super) fn agent_folder(state: &Mutex<Backend>, agent_id: &str) -> Result<String, HttpReply> {
    let entry = crate::transport::rest::resolve_entry(state, agent_id)?;
    Ok(entry.folder)
}

/// Read a per-worker module state and extract a specific array field,
/// unwrapping the wrapper object the TUI persists.
pub(super) fn unwrap_module_array(
    state: &Mutex<Backend>,
    agent_id: &str,
    query: &str,
    module_key: &str,
    array_field: &str,
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
        None => match backend.inspect_mut().list_workers(folder_path) {
            Ok(w) => match w.first() {
                Some(w) => w.clone(),
                None => return HttpReply::error(404, "no workers found"),
            },
            Err(_) => return HttpReply::error(404, "cannot list workers"),
        },
    };
    match backend.inspect_mut().read_worker(folder_path, &wid) {
        Ok(ws) => {
            let arr = ws
                .get("modules")
                .and_then(|m| m.get(module_key))
                .and_then(|d| d.get(array_field))
                .cloned()
                .unwrap_or(serde_json::json!([]));
            HttpReply::ok(&arr)
        }
        Err(_) => HttpReply::error(404, "worker state unavailable"),
    }
}

/// Read a shared YAML map and transform each entry via a closure into an array.
pub(super) fn yaml_map_to_keyed_array<F>(
    state: &Mutex<Backend>,
    folder: &str,
    filename: &str,
    transform: F,
) -> HttpReply
where
    F: Fn(String, serde_json::Value) -> serde_json::Value,
{
    let folder_path = Path::new(folder);
    let Ok(mut backend) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let bytes = backend.inspect_mut().read_shared(folder_path, filename);
    match bytes {
        Ok(raw) => {
            let yaml_val: Result<serde_json::Value, _> = serde_yaml::from_slice(&raw);
            match yaml_val {
                Ok(serde_json::Value::Object(map)) => {
                    let items: Vec<serde_json::Value> =
                        map.into_iter().map(|(k, v)| transform(k, v)).collect();
                    HttpReply::ok(&items)
                }
                Ok(_) => HttpReply::ok(&serde_json::json!([])),
                Err(_) => HttpReply::error(502, "YAML parse failed"),
            }
        }
        Err(_) => HttpReply::ok(&serde_json::json!([])),
    }
}

/// Extract the `worker` query parameter from a raw query string.
pub(super) fn extract_worker_param(query: &str) -> Option<String> {
    query.split('&').filter(|s| !s.is_empty()).find_map(|pair| {
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
