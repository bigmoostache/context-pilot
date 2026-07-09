//! Read-only **inspection** of an agent's on-disk state (design doc §3.1,
//! tier-② files).
//!
//! The orchestration backend serves rich agent data (threads, memory, todos,
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

/// The global shared configuration file (threads, memory, tree, logs, etc.).
const CONFIG_FILE: &str = "config.json";

/// Directory holding per-worker state files (`<worker_id>.json`).
const STATES_DIR: &str = "states";

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
struct AgentCache {
    /// Cached parse of `config.json`.
    config: Option<CachedJson>,

    /// Cached parses of `states/<worker>.json`, keyed by worker id.
    workers: HashMap<String, CachedJson>,
}

// ── StateReader ────────────────────────────────────────────────────────

/// Read-only, mtime-cached reader of agent persistence files.
///
/// One [`StateReader`] serves the whole fleet; internally it maintains a
/// per-agent [`AgentCache`] so repeated requests for the same agent amortise
/// the file I/O across the mtime-check fast path.
#[derive(Debug, Default)]
pub struct StateReader {
    /// Per-agent caches, keyed by the agent's **folder** (canonical working
    /// directory).
    agents: HashMap<PathBuf, AgentCache>,
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
    /// Returns [`io::Error`] only when both the fresh read *and* the cache miss
    /// — i.e. the file has never been successfully read for this agent.
    pub fn read_config(&mut self, folder: &Path) -> io::Result<Value> {
        let path = folder.join(CP_DIR).join(CONFIG_FILE);
        let cache = self.agents.entry(folder.to_path_buf()).or_default();
        read_cached_json(&path, &mut cache.config)
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
    /// # Errors
    ///
    /// Returns [`io::Error`] if the `states/` directory cannot be listed.
    /// A missing directory yields an empty list.
    pub fn list_workers(&self, folder: &Path) -> io::Result<Vec<String>> {
        let dir = folder.join(CP_DIR).join(STATES_DIR);
        list_json_stems(&dir)
    }

}

// ── Helpers ────────────────────────────────────────────────────────────

/// Read and parse a JSON file, returning the cached value on mtime-hit or
/// torn-read fallback.
fn read_cached_json(path: &Path, slot: &mut Option<CachedJson>) -> io::Result<Value> {
    let current_mtime = file_mtime(path)?;

    // Cache hit: mtime unchanged → return the existing parse.
    if let Some(cached) = slot.as_ref() {
        if cached.mtime == current_mtime {
            return Ok(cached.data.clone());
        }
    }

    // Cache miss: read + parse.
    match read_json(path) {
        Ok(data) => {
            *slot = Some(CachedJson { mtime: current_mtime, data: data.clone() });
            Ok(data)
        }
        Err(_) if slot.is_some() => {
            // Torn read while the agent is mid-write → return last good value.
            Ok(slot.as_ref().map(|c| c.data.clone()).unwrap_or(Value::Null))
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
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if let Some(stem) = name.strip_suffix(".json") {
            stems.push(stem.to_owned());
        }
    }
    stems.sort();
    Ok(stems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_config(folder: &Path, value: &Value) {
        let dir = folder.join(CP_DIR);
        fs::create_dir_all(&dir).expect("mkdir");
        fs::write(dir.join(CONFIG_FILE), serde_json::to_vec(value).expect("ser")).expect("write");
    }

    fn write_worker(folder: &Path, worker: &str, value: &Value) {
        let dir = folder.join(CP_DIR).join(STATES_DIR);
        fs::create_dir_all(&dir).expect("mkdir");
        fs::write(dir.join(format!("{worker}.json")), serde_json::to_vec(value).expect("ser")).expect("write");
    }

    #[test]
    fn read_config_caches_and_returns_on_mtime_hit() {
        let dir = tempdir().expect("dir");
        let folder = dir.path();
        let val = serde_json::json!({"modules": {"threads": []}});
        write_config(folder, &val);

        let mut reader = StateReader::new();
        let first = reader.read_config(folder).expect("read");
        assert_eq!(first, val);

        // Second read without file change → mtime hit, returns same.
        let second = reader.read_config(folder).expect("read");
        assert_eq!(second, val);
    }

    #[test]
    fn read_config_re_parses_on_mtime_change() {
        let dir = tempdir().expect("dir");
        let folder = dir.path();
        let v1 = serde_json::json!({"version": 1});
        write_config(folder, &v1);

        let mut reader = StateReader::new();
        let _first = reader.read_config(folder).expect("read");

        // Mutate the file (mtime changes).
        let v2 = serde_json::json!({"version": 2});
        // Ensure mtime actually differs (some filesystems have 1s granularity).
        std::thread::sleep(std::time::Duration::from_millis(50));
        write_config(folder, &v2);

        let second = reader.read_config(folder).expect("read");
        assert_eq!(second, v2);
    }

    #[test]
    fn read_config_falls_back_on_torn_read() {
        let dir = tempdir().expect("dir");
        let folder = dir.path();
        let good = serde_json::json!({"ok": true});
        write_config(folder, &good);

        let mut reader = StateReader::new();
        let _first = reader.read_config(folder).expect("read");

        // Corrupt the file (simulates mid-write torn read).
        std::thread::sleep(std::time::Duration::from_millis(50));
        let path = folder.join(CP_DIR).join(CONFIG_FILE);
        fs::write(&path, b"{{{{not json").expect("write");

        // Should fall back to the last good value.
        let fallback = reader.read_config(folder).expect("read");
        assert_eq!(fallback, good);
    }

    #[test]
    fn read_worker_and_list_workers() {
        let dir = tempdir().expect("dir");
        let folder = dir.path();
        let w1 = serde_json::json!({"modules": {"todo": {}}});
        let w2 = serde_json::json!({"modules": {"spine": {}}});
        write_worker(folder, "abc123", &w1);
        write_worker(folder, "def456", &w2);

        let mut reader = StateReader::new();
        let workers = reader.list_workers(folder).expect("list");
        assert_eq!(workers, vec!["abc123", "def456"]);

        let read = reader.read_worker(folder, "abc123").expect("read");
        assert_eq!(read, w1);
    }

    #[test]
    fn list_workers_on_missing_dir_returns_empty() {
        let dir = tempdir().expect("dir");
        let reader = StateReader::new();
        let workers = reader.list_workers(dir.path()).expect("list");
        assert!(workers.is_empty());
    }

}
