//! One-shot boot migration of the pre-T860 "threads" layout to "agents".
//!
//! Rewrites, under the store dir:
//! - JSON keys `thread_id`, `focused_thread_id`, `threads_worker`, `threads`, …
//!   to their `agent` names (see [`KEY_RENAMES`]);
//! - the `"threads"` module id / panel kind to `"agents"`;
//! - agent ids `T<n>` to `A<n>`, but only in id-bearing fields ([`ID_KEYS`]),
//!   agent records ([`AGENT_RECORDS`]) and maps keyed by agent id
//!   ([`ID_MAPS`]). Free text (messages, logs, todo names) and tool ids, which
//!   also look like `T<n>`, are untouched;
//! - `threads/T<n>.json` to `agents/A<n>.json` and `states/T<n>.json` to
//!   `states/A<n>.json`.
//!
//! `config.json` is written last, so it doubles as the commit marker: once it
//! carries `modules.agents` the migration is done and every later boot skips
//! it. An interrupted run is simply redone (every step is idempotent).

use std::fs;
use std::io;
use std::path::Path;

use serde_json::{Map, Value};

/// Old key → new key, applied at every depth.
const KEY_RENAMES: &[(&str, &str)] = &[
    ("thread_id", "agent_id"),
    ("thread_name", "agent_name"),
    ("source_thread_id", "source_agent_id"),
    ("focused_thread_id", "focused_agent_id"),
    ("drilled_thread_id", "drilled_agent_id"),
    ("selected_thread_idx", "selected_agent_idx"),
    ("creating_thread", "creating_agent"),
    ("new_thread_title", "new_agent_title"),
    ("threads_worker", "agents_worker"),
    ("threads", "agents"),
];

/// Keys (after renaming) whose string value is an agent id.
///
/// Bare `id` is NOT here: tool-use ids are `T<n>` too and sit under `id` in
/// conversation panels. Agent records are handled by [`AGENT_RECORDS`].
const ID_KEYS: &[&str] = &[
    "agent_id",
    "source_agent_id",
    "focused_agent_id",
    "drilled_agent_id",
    "notified_my_turn_id",
    "focus_file",
    "worker_id",
];

/// Parent keys (after renaming) whose elements are agent records, so their
/// `id` is an agent id. Agent message files get the same treatment via the
/// `agent_file` flag of [`rewrite_file`].
const AGENT_RECORDS: &[&str] = &["agents"];

/// Keys (after renaming) holding an object keyed by agent id.
const ID_MAPS: &[&str] = &["last_read_count"];

/// Keys (after renaming) whose `"threads"` string values name the module/kind.
const KIND_KEYS: &[&str] = &["panel_type", "active_modules"];

/// `T<n>` → `A<n>`; anything else unchanged.
#[must_use]
pub fn upgrade_id(id: &str) -> String {
    match id.strip_prefix('T') {
        Some(n) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => format!("A{n}"),
        _ => id.to_owned(),
    }
}

/// New name of a key.
fn rename_key(key: &str) -> &str {
    KEY_RENAMES.iter().find(|&&(old, _)| old == key).map_or(key, |&(_, new)| new)
}

/// Rewrite a scalar found under `key`.
fn rewrite_scalar(key: &str, value: &mut Value) {
    let Some(s) = value.as_str() else { return };
    let new = if ID_KEYS.contains(&key) {
        upgrade_id(s)
    } else if KIND_KEYS.contains(&key) && s == "threads" {
        "agents".to_owned()
    } else {
        return;
    };
    *value = Value::String(new);
}

/// Rewrite an object: rename keys, re-key id maps, recurse.
fn rewrite_object(parent: &str, map: &mut Map<String, Value>) {
    let id_map = ID_MAPS.contains(&parent);
    let old = std::mem::take(map);
    for (k, mut v) in old {
        let key = if id_map { upgrade_id(&k) } else { rename_key(&k).to_owned() };
        upgrade_value(&key, &mut v);
        let _prev = map.insert(key, v);
    }
    if AGENT_RECORDS.contains(&parent) {
        rewrite_record_id(map);
    }
}

/// `T<n>` → `A<n>` on the `id` of an agent record.
fn rewrite_record_id(map: &mut Map<String, Value>) {
    if let Some(new) = map.get("id").and_then(Value::as_str).map(upgrade_id) {
        let _prev = map.insert("id".to_owned(), Value::String(new));
    }
}

/// Rewrite `value`, found under `key` (arrays pass their key to elements).
pub fn upgrade_value(key: &str, value: &mut Value) {
    if let Some(map) = value.as_object_mut() {
        rewrite_object(key, map);
    } else if let Some(items) = value.as_array_mut() {
        for v in items {
            upgrade_value(key, v);
        }
    } else {
        rewrite_scalar(key, value);
    }
}

/// Whether `config` still has the pre-T860 layout.
#[must_use]
pub fn needs_migration(config: &Value) -> bool {
    config.get("modules").is_some_and(|m| m.get("threads").is_some() && m.get("agents").is_none())
}

/// Read, migrate and write `src` to `dst` (removing `src` when they differ).
///
/// `agent_file`: the file is an agent message file whose top-level `id` is
/// the agent id.
fn rewrite_file(src: &Path, dst: &Path, agent_file: bool) -> io::Result<()> {
    let bytes = fs::read(src)?;
    let Ok(mut v) = serde_json::from_slice::<Value>(&bytes) else { return Ok(()) };
    upgrade_value("", &mut v);
    if agent_file && let Some(map) = v.as_object_mut() {
        rewrite_record_id(map);
    }
    fs::write(dst, serde_json::to_vec_pretty(&v).map_err(io::Error::other)?)?;
    if src != dst {
        fs::remove_file(src)?;
    }
    Ok(())
}

/// Migrate every `*.json` of `src_dir` into `dst_dir`, renaming `T<n>.json`.
fn rewrite_dir(src_dir: &Path, dst_dir: &Path, agent_files: bool) -> io::Result<()> {
    let Ok(entries) = fs::read_dir(src_dir) else { return Ok(()) };
    fs::create_dir_all(dst_dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        rewrite_file(&path, &dst_dir.join(format!("{}.json", upgrade_id(stem))), agent_files)?;
    }
    Ok(())
}

/// Run the migration on the store at `root` if it still has the old layout.
///
/// Returns whether anything was migrated.
///
/// # Errors
/// Any I/O failure; the store is then left half-migrated and the next boot
/// resumes (config.json is rewritten last).
pub fn upgrade_store(root: &Path) -> io::Result<bool> {
    let config_path = root.join("config.json");
    let Ok(bytes) = fs::read(&config_path) else { return Ok(false) };
    let Ok(config) = serde_json::from_slice::<Value>(&bytes) else { return Ok(false) };
    if !needs_migration(&config) {
        return Ok(false);
    }
    rewrite_dir(&root.join("threads"), &root.join("agents"), true)?;
    let _empty = fs::remove_dir(root.join("threads"));
    for dir in ["states", "modules", "panels"] {
        rewrite_dir(&root.join(dir), &root.join(dir), false)?;
    }
    rewrite_file(&config_path, &config_path, false)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Migrate a value as if it were a whole file.
    fn run(mut v: Value) -> Value {
        upgrade_value("", &mut v);
        v
    }

    #[test]
    fn ids_only_in_id_fields() {
        assert_eq!(upgrade_id("T860"), "A860");
        assert_eq!(upgrade_id("T"), "T");
        assert_eq!(upgrade_id("T86x"), "T86x");
        assert_eq!(upgrade_id("main_worker"), "main_worker");
    }

    #[test]
    fn config_agents_and_keys() {
        let v = run(json!({"modules": {
            "core": {"active_modules": ["threads", "todo"]},
            "threads": {"focus_file": "T7",
                        "threads": [{"id": "T7", "name": "T9 talk", "origin": {"thread_id": "T3"}}]},
            "scratchpad": {"scratchpad_cells": [{"id": "C1", "thread_id": "T7"}]}
        }}));
        let want = json!({"modules": {
            "core": {"active_modules": ["agents", "todo"]},
            "agents": {"focus_file": "A7",
                       "agents": [{"id": "A7", "name": "T9 talk", "origin": {"agent_id": "A3"}}]},
            "scratchpad": {"scratchpad_cells": [{"id": "C1", "agent_id": "A7"}]}
        }});
        assert_eq!(v, want);
    }

    #[test]
    fn worker_state_and_tool_ids() {
        let v = run(json!({
            "worker_id": "T860",
            "important_panel_uids": {"threads": "UID_1_P"},
            "modules": {"threads_worker": {"focused_thread_id": "T2", "last_read_count": {"T2": 4u64},
                                           "selected_thread_idx": 1u64}},
            "messages": [{"id": "T924", "tool_uses": [{"id": "T925"}]}]
        }));
        let want = json!({
            "worker_id": "A860",
            "important_panel_uids": {"agents": "UID_1_P"},
            "modules": {"agents_worker": {"focused_agent_id": "A2", "last_read_count": {"A2": 4u64},
                                          "selected_agent_idx": 1u64}},
            "messages": [{"id": "T924", "tool_uses": [{"id": "T925"}]}]
        });
        assert_eq!(v, want);
    }

    #[test]
    fn panel_kind() {
        let v = run(json!({"panel_type": "threads", "name": "threads"}));
        assert_eq!(v, json!({"panel_type": "agents", "name": "threads"}));
    }

    /// Write a pre-T860 store under `root`.
    fn seed_old_store(root: &Path) {
        for d in ["threads", "states", "modules", "panels"] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        let files = [
            ("config.json", json!({"modules": {"threads": {"threads": [{"id": "T1"}]}}})),
            ("threads/T1.json", json!({"id": "T1", "messages": [{"content": "T1"}]})),
            ("states/T1.json", json!({"worker_id": "T1"})),
            ("modules/todo.json", json!({"todos": [{"id": "X1", "thread_id": "T1"}]})),
        ];
        for (p, v) in files {
            fs::write(root.join(p), v.to_string()).unwrap();
        }
    }

    /// Parse the JSON file at `root/p`.
    fn read(root: &Path, p: &str) -> Value {
        serde_json::from_slice(&fs::read(root.join(p)).unwrap()).unwrap()
    }

    /// Assert the store at `root` holds the migrated form of [`seed_old_store`].
    fn assert_migrated(root: &Path) {
        let expected = [
            ("agents/A1.json", json!({"id": "A1", "messages": [{"content": "T1"}]})),
            ("states/A1.json", json!({"worker_id": "A1"})),
            ("modules/todo.json", json!({"todos": [{"id": "X1", "agent_id": "A1"}]})),
            ("config.json", json!({"modules": {"agents": {"agents": [{"id": "A1"}]}}})),
        ];
        for (p, want) in expected {
            assert_eq!(read(root, p), want, "{p}");
        }
        assert!(!root.join("threads").exists());
        assert!(!root.join("states/T1.json").exists());
    }

    #[test]
    fn store_roundtrip_is_idempotent() {
        let root = std::env::temp_dir().join(format!("cp-migrate-{}", std::process::id()));
        let _before = fs::remove_dir_all(&root);
        seed_old_store(&root);
        assert!(upgrade_store(&root).unwrap());
        assert_migrated(&root);
        assert!(!upgrade_store(&root).unwrap());
        let _after = fs::remove_dir_all(&root);
    }
}
