//! Read-only **inspection** of an agent's on-disk state (design doc §3.1,
//! tier-② files).
//!
//! The orchestration backend serves rich agent data (agents, memory, todos,
//! panels, finder trees…) to the web frontend. Rather than bloating the oplog
//! with every piece of mutable state, the **inspection plane** reads the
//! agent's own persistence files — the same ones the agent's
//! [`PersistenceWriter`] coalesces on a 50 ms cadence — and reshapes them to
//! the JSON shapes the frontend expects.
//!
//! # Mtime memo-cache
//!
//! `config.json` is ~386 KB; re-parsing it on every HTTP request would waste
//! CPU. [`StateReader`] caches the most recent parse keyed by the file's
//! `mtime`: a [`stat`](std::fs::metadata) call (~1 µs) gates whether the
//! heavier read + parse (~1 ms) runs. In the common case (agent hasn't saved
//! since the last request) the reader returns a clone of the cached
//! [`Value`](serde_json::Value) with no I/O beyond the `stat`.
//!
//! # Concurrency note
//!
//! The agent may be mid-write when we read. A torn `config.json` will fail
//! JSON parse; the reader returns the **last good cached value** in that case,
//! so a transient write never surfaces as an error to the frontend — it just
//! means the response is one save-cycle stale, which is indistinguishable from
//! normal cadence.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

/// Subdirectory the agent stores its persistence files in.
const CP_DIR: &str = ".context-pilot";

/// The global shared configuration file (agents, memory, tree, logs, etc.).
const CONFIG_FILE: &str = "config.json";

/// Directory holding per-worker state files (`<worker_id>.json`).
const STATES_DIR: &str = "states";

/// Directory holding per-agent message files (`<agent_id>.json`). The agent
/// keeps only slim agent metadata in `config.json`; messages live here.
const AGENTS_DIR: &str = "agents";

/// Directory holding per-group message files (`<group_id>.json`). Since T860
/// C3 an agent's conversation lives in its human group's file.
const GROUPS_DIR: &str = "groups";

/// Directory holding per-agent tool-call traces (`<agent_id>.json`). Since
/// T860 H8 they live here, not in the human group, and are interleaved with
/// the group's messages by timestamp when served.
const ACTIVITY_DIR: &str = "activity";

/// Pre-T860 name of [`AGENTS_DIR`], read when the agent binary is older.
const LEGACY_AGENTS_DIR: &str = "threads";

/// Directory holding large global modules saved outside `config.json`
/// (`<module_id>.json`, e.g. `tree`, `todo`), rewritten only when they change.
const MODULES_DIR: &str = "modules";

// ── Cached value ───────────────────────────────────────────────────────

/// A parsed JSON value paired with the file's `mtime` at the time of parsing,
/// so the reader can skip re-parsing when the file hasn't changed.
#[derive(Clone, Debug)]
struct CachedJson {
    /// The `mtime` observed when `data` was parsed.
    mtime: SystemTime,
    /// The parsed file contents.
    data: Value,
}

// ── Per-agent file cache ───────────────────────────────────────────────

/// Cached state for one agent's `.context-pilot/` directory.
#[derive(Debug, Default)]
struct FleetCache {
    /// Cached parse of `config.json`.
    config: Option<CachedJson>,

    /// Cached parses of `states/<worker>.json`, keyed by worker id.
    workers: HashMap<String, CachedJson>,

    /// Cached parses of `agents/<id>.json`, keyed by agent id.
    agents: HashMap<String, CachedJson>,

    /// Cached parses of `modules/<id>.json`, keyed by module id.
    modules: HashMap<String, CachedJson>,
}

// ── StateReader ────────────────────────────────────────────────────────

/// Read-only, mtime-cached reader of agent persistence files.
///
/// One [`StateReader`] serves the whole fleet; internally it maintains a
/// per-agent [`FleetCache`] so repeated requests for the same agent amortise
/// the file I/O across the mtime-check fast path.
#[derive(Debug, Default)]
pub struct StateReader {
    /// Per-agent caches, keyed by the agent's **folder** (canonical working
    /// directory).
    agents: HashMap<PathBuf, FleetCache>,
}

impl StateReader {
    /// Create an empty reader (no agents cached yet).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Read and cache-parse the agent's `config.json` (global shared config).
    ///
    /// Returns the cached value on mtime-hit, re-parses on mtime-miss, and
    /// falls back to the last good parse on a torn read (agent mid-write).
    ///
    /// # Errors
    ///
    /// Agent entries stored slim (no `messages` key) get their messages
    /// spliced back from `agents/<id>.json`, so callers see the full shape.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] only when both the fresh read *and* the cache miss
    /// — i.e. the file has never been successfully read for this agent.
    pub fn read_config(&mut self, folder: &Path) -> io::Result<Value> {
        let path = folder.join(CP_DIR).join(CONFIG_FILE);
        let cache = self.agents.entry(folder.to_path_buf()).or_default();
        let mut config = read_cached_json(&path, &mut cache.config)?;
        let legacy = normalize_legacy_agents(&mut config);
        let dir = if legacy { LEGACY_AGENTS_DIR } else { AGENTS_DIR };
        let cp = folder.join(CP_DIR);
        hydrate_agent_messages(
            &MessageDirs { groups: &cp.join(GROUPS_DIR), agents: &cp.join(dir), activity: &cp.join(ACTIVITY_DIR) },
            &mut config,
            &mut cache.agents,
        );
        overlay_module_files(&folder.join(CP_DIR).join(MODULES_DIR), &mut config, &mut cache.modules);
        Ok(config)
    }

    /// Read and cache-parse a single worker's state file.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] on first-read failure (no cached fallback yet).
    pub fn read_worker(&mut self, folder: &Path, worker_id: &str) -> io::Result<Value> {
        let path = folder.join(CP_DIR).join(STATES_DIR).join(format!("{worker_id}.json"));
        let cache = self.agents.entry(folder.to_path_buf()).or_default();
        let slot = cache
            .workers
            .entry(worker_id.to_owned())
            .or_insert_with(|| CachedJson { mtime: SystemTime::UNIX_EPOCH, data: Value::Null });
        read_cached_json_slot(&path, slot)
    }

    /// List the worker ids for an agent (filenames in `states/` sans `.json`).
    ///
    /// Worker enumeration is deliberately **uncached** (it reads the `states/`
    /// directory fresh each call), so unlike [`read_config`](Self::read_config)
    /// and [`read_worker`](Self::read_worker) it needs no `&self` — it is an
    /// associated function on [`StateReader`], reached as
    /// `StateReader::list_workers(folder)`.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if the `states/` directory cannot be listed.
    /// A missing directory yields an empty list.
    pub fn list_workers(folder: &Path) -> io::Result<Vec<String>> {
        let dir = folder.join(CP_DIR).join(STATES_DIR);
        list_json_stems(&dir)
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

/// Splice each `modules/<id>.json` into `config.modules.<id>`, replacing any
/// inline copy, so callers keep reading one merged `config` shape. Unreadable
/// files are skipped (the inline copy, if any, stays).
fn overlay_module_files(modules_dir: &Path, config: &mut Value, cache: &mut HashMap<String, CachedJson>) {
    let Ok(entries) = fs::read_dir(modules_dir) else { return };
    let Some(modules) = config.get_mut("modules").and_then(Value::as_object_mut) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else { continue };
        let slot =
            cache.entry(id.clone()).or_insert_with(|| CachedJson { mtime: SystemTime::UNIX_EPOCH, data: Value::Null });
        if let Ok(value) = read_cached_json_slot(&path, slot) {
            let _prev = modules.insert(id, value);
        }
    }
}

/// Rename a pre-T860 `modules.threads.threads` to `modules.agents.agents` in
/// memory, so readers only know the new shape. Returns whether it was legacy.
fn normalize_legacy_agents(config: &mut Value) -> bool {
    let Some(modules) = config.get_mut("modules").and_then(Value::as_object_mut) else { return false };
    if modules.contains_key("agents") {
        return false;
    }
    let Some(mut module) = modules.remove("threads") else { return false };
    if let Some(obj) = module.as_object_mut()
        && let Some(list) = obj.remove("threads")
    {
        let _prev = obj.insert("agents".to_owned(), list);
    }
    let _prev = modules.insert("agents".to_owned(), module);
    true
}

/// Where agent messages may live on disk.
struct MessageDirs<'dirs> {
    /// `groups/` — current layout, one file per group.
    groups: &'dirs Path,
    /// `agents/` (or legacy `threads/`) — pre-groups per-agent files.
    agents: &'dirs Path,
    /// `activity/` — per-agent tool-call traces (T860 H8).
    activity: &'dirs Path,
}

/// Splice `messages` into every agent entry of `config` lacking them: from
/// `groups/<human_group_id>.json` when the agent has a human group, else from
/// the pre-groups `agents/<id>.json`, then interleave the agent's
/// `activity/<id>.json` traces by timestamp. Legacy configs (inline messages)
/// pass through as-is; a missing/unreadable file contributes nothing.
fn hydrate_agent_messages(dirs: &MessageDirs<'_>, config: &mut Value, cache: &mut HashMap<String, CachedJson>) {
    let Some(entries) = config.pointer_mut("/modules/agents/agents").and_then(Value::as_array_mut) else {
        return;
    };
    for entry in entries {
        let Some(obj) = entry.as_object_mut() else { continue };
        if obj.contains_key("messages") {
            continue;
        }
        let Some(id) = obj.get("id").and_then(Value::as_str).map(str::to_owned) else { continue };
        let group = obj.get("human_group_id").and_then(Value::as_str).filter(|g| !g.is_empty()).map(str::to_owned);
        let (key, path) = group.map_or_else(
            || (id.clone(), dirs.agents.join(format!("{id}.json"))),
            |gid| (format!("group:{gid}"), dirs.groups.join(format!("{gid}.json"))),
        );
        let chat = cached_messages(cache, key, &path);
        let traces = cached_messages(cache, format!("activity:{id}"), &dirs.activity.join(format!("{id}.json")));
        if let Some(messages) = interleave(chat, traces) {
            let _prev = obj.insert("messages".to_owned(), Value::Array(messages));
        }
    }
}

/// The `messages` array of the JSON file at `path`, through the per-key cache
/// (`None` when the file is missing, unreadable or has no array).
fn cached_messages(cache: &mut HashMap<String, CachedJson>, key: String, path: &Path) -> Option<Vec<Value>> {
    let slot = cache.entry(key).or_insert_with(|| CachedJson { mtime: SystemTime::UNIX_EPOCH, data: Value::Null });
    let file = read_cached_json_slot(path, slot).ok()?;
    file.get("messages").and_then(Value::as_array).cloned()
}

/// Stable merge of chat messages and tool traces by `timestamp`, chat first
/// on ties. `None` when neither source exists.
fn interleave(chat: Option<Vec<Value>>, traces: Option<Vec<Value>>) -> Option<Vec<Value>> {
    let (mut all, extra) = match (chat, traces) {
        (None, None) => return None,
        (Some(c), None) => return Some(c),
        (None, Some(t)) => return Some(t),
        (Some(c), Some(t)) => (c, t),
    };
    all.extend(extra);
    all.sort_by_key(|m| m.get("timestamp").and_then(Value::as_u64).unwrap_or(0));
    Some(all)
}

/// Read and parse a JSON file, returning the cached value on mtime-hit or
/// torn-read fallback.
fn read_cached_json(path: &Path, slot: &mut Option<CachedJson>) -> io::Result<Value> {
    let current_mtime = file_mtime(path)?;

    // Cache hit: mtime unchanged → return the existing parse.
    if let Some(cached) = slot.as_ref()
        && cached.mtime == current_mtime
    {
        return Ok(cached.data.clone());
    }

    // Cache miss: read + parse.
    match read_json(path) {
        Ok(data) => {
            *slot = Some(CachedJson { mtime: current_mtime, data: data.clone() });
            Ok(data)
        }
        Err(_) if slot.is_some() => {
            // Torn read while the agent is mid-write → return last good value.
            Ok(slot.as_ref().map_or(Value::Null, |c| c.data.clone()))
        }
        Err(e) => Err(e),
    }
}

/// Read and parse a JSON file into an existing cache slot, with torn-read
/// fallback to the slot's current value.
fn read_cached_json_slot(path: &Path, slot: &mut CachedJson) -> io::Result<Value> {
    let current_mtime = match file_mtime(path) {
        Ok(mt) => mt,
        Err(_) if slot.data != Value::Null => return Ok(slot.data.clone()),
        Err(e) => return Err(e),
    };

    if slot.mtime == current_mtime {
        return Ok(slot.data.clone());
    }

    match read_json(path) {
        Ok(data) => {
            slot.mtime = current_mtime;
            slot.data = data.clone();
            Ok(data)
        }
        Err(_) if slot.data != Value::Null => Ok(slot.data.clone()),
        Err(e) => Err(e),
    }
}

/// Read and parse a JSON file in one shot.
fn read_json(path: &Path) -> io::Result<Value> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::other(format!("parse {}: {e}", path.display())))
}

/// Get a file's modification time.
fn file_mtime(path: &Path) -> io::Result<SystemTime> {
    fs::metadata(path)?.modified()
}

/// List `.json` file stems in a directory (e.g. `["abc123", "def456"]` from
/// `states/abc123.json` and `states/def456.json`).
fn list_json_stems(dir: &Path) -> io::Result<Vec<String>> {
    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut stems = Vec::new();
    for entry in read_dir {
        let dir_entry = entry?;
        let name = dir_entry.file_name();
        let Some(name_str) = name.to_str() else { continue };
        if let Some(stem) = name_str.strip_suffix(".json") {
            stems.push(stem.to_owned());
        }
    }
    stems.sort();
    Ok(stems)
}

#[cfg(test)]
mod tests;
