//! Orchestrator-owned **retired-agent store** (T271).
//!
//! Retiring an agent stops its process *without* deleting its realm folder, so
//! the agent can be brought back trivially later. Because a retired agent's
//! process is **dead**, it cannot hold this state itself — the orchestrator
//! owns it, persisted in a single JSON file alongside the registry records.
//!
//! It lives in [`services`](super) beside the other pure, single-owner backend
//! data structures (the cost breaker, the materialized view): like them it owns
//! no I/O thread, just in-memory state with a disk-backed persistence helper.
//!
//! # Why a separate store (not the registry `Entry`)
//!
//! The registry record (`<agents_dir>/<id>.json`) is written by the *agent* at
//! boot and removed on its clean shutdown — exactly the moment retiring kills
//! it. So the live registry can't be the source of truth for "retired", and it
//! can't carry the snapshot needed to render a retired card whose agent is no
//! longer running. This store keeps that snapshot independently, surviving both
//! the agent's death and a backend restart.
//!
//! # File shape
//!
//! `<agents_dir>/retired.json` — a JSON array of [`RetiredRecord`]. The fleet
//! scans ignore it (it does not deserialize as a registry `Entry`, so the
//! guarded scan loops skip it). Writes are atomic (`tmp` → `rename`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The display + identity snapshot of a retired agent.
///
/// Captured at retire time so the dashboard's Retired section can render the
/// card without a live process or registry record to read from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetiredRecord {
    /// Stable agent id (folder-derived) — the key the endpoints address.
    pub id: String,
    /// Display name (the realm folder's basename).
    pub name: String,
    /// Absolute realm folder — kept intact; used to block same-path creation
    /// and to respawn the agent on unretire.
    pub folder: String,
    /// API model name at retire time (informational on the card).
    pub model: String,
    /// Wire provider id at retire time (informational on the card).
    pub provider: String,
    /// Epoch-ms the agent was retired.
    pub retired_at_ms: u64,
}

/// In-memory + on-disk set of retired agents, keyed by agent id.
#[derive(Debug, Default)]
pub struct RetiredStore {
    /// Retired agents by id.
    records: HashMap<String, RetiredRecord>,
    /// The backing file (`<agents_dir>/retired.json`).
    path: PathBuf,
}

impl RetiredStore {
    /// Load the store from `<agents_dir>/retired.json`, or start empty.
    ///
    /// A missing or unreadable file yields an empty store — retiring is a
    /// best-effort convenience, never a hard dependency for the backend to boot.
    #[must_use]
    pub fn load(agents_dir: &Path) -> Self {
        let path = agents_dir.join("retired.json");
        let records = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<RetiredRecord>>(&bytes).ok())
            .map(|list| list.into_iter().map(|r| (r.id.clone(), r)).collect())
            .unwrap_or_default();
        Self { records, path }
    }

    /// Whether the agent with this id is retired.
    #[must_use]
    pub fn is_retired(&self, id: &str) -> bool {
        self.records.contains_key(id)
    }

    /// Whether any retired agent owns this realm folder.
    ///
    /// Drives the create guard (requirement 4): a new agent must not be spawned
    /// in a folder a retired agent still owns.
    #[must_use]
    pub fn is_folder_retired(&self, folder: &str) -> bool {
        self.records.values().any(|r| r.folder == folder)
    }

    /// All retired records (for the dashboard's Retired section).
    #[must_use]
    pub fn list(&self) -> Vec<RetiredRecord> {
        let mut v: Vec<RetiredRecord> = self.records.values().cloned().collect();
        // Most-recently-retired first.
        v.sort_by(|a, b| b.retired_at_ms.cmp(&a.retired_at_ms));
        v
    }

    /// Mark an agent retired and persist. Replaces any prior record for the id.
    pub fn retire(&mut self, record: RetiredRecord) {
        let _prev = self.records.insert(record.id.clone(), record);
        self.persist();
    }

    /// Clear an agent's retired flag, returning its record if it was retired.
    pub fn unretire(&mut self, id: &str) -> Option<RetiredRecord> {
        let removed = self.records.remove(id);
        if removed.is_some() {
            self.persist();
        }
        removed
    }

    /// Atomically write the store to disk (`tmp` → `rename`).
    ///
    /// A write failure is logged and swallowed: the in-memory state stays
    /// authoritative for this process; the worst case is a retired flag lost
    /// across a backend restart, never a crash.
    fn persist(&self) {
        let mut list: Vec<&RetiredRecord> = self.records.values().collect();
        list.sort_by(|a, b| a.id.cmp(&b.id));
        let Ok(bytes) = serde_json::to_vec_pretty(&list) else {
            eprintln!("retire: serialize failed");
            return;
        };
        let tmp = self.path.with_extension("json.tmp");
        if std::fs::write(&tmp, &bytes).is_err() {
            eprintln!("retire: write tmp failed: {}", tmp.display());
            return;
        }
        if let Err(e) = std::fs::rename(&tmp, &self.path) {
            eprintln!("retire: rename failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, folder: &str) -> RetiredRecord {
        RetiredRecord {
            id: id.to_owned(),
            name: "n".to_owned(),
            folder: folder.to_owned(),
            model: "m".to_owned(),
            provider: "anthropic".to_owned(),
            retired_at_ms: 1,
        }
    }

    #[test]
    fn retire_unretire_roundtrips_through_disk() {
        let dir = std::env::temp_dir().join(format!("cp-retire-test-{}", std::process::id()));
        drop(std::fs::create_dir_all(&dir));

        let mut store = RetiredStore::load(&dir);
        assert!(!store.is_retired("a"));
        store.retire(rec("a", "/tmp/a"));
        assert!(store.is_retired("a"));
        assert!(store.is_folder_retired("/tmp/a"));
        assert!(!store.is_folder_retired("/tmp/b"));

        // Reload from disk proves persistence.
        let reloaded = RetiredStore::load(&dir);
        assert!(reloaded.is_retired("a"));
        assert_eq!(reloaded.list().len(), 1);

        // Unretire clears it.
        let mut store2 = RetiredStore::load(&dir);
        let removed = store2.unretire("a");
        assert!(removed.is_some());
        assert!(!store2.is_retired("a"));
        assert!(RetiredStore::load(&dir).list().is_empty());

        drop(std::fs::remove_dir_all(&dir));
    }

    #[test]
    fn unretire_unknown_is_none() {
        let dir = std::env::temp_dir().join(format!("cp-retire-test2-{}", std::process::id()));
        drop(std::fs::create_dir_all(&dir));
        let mut store = RetiredStore::load(&dir);
        assert!(store.unretire("ghost").is_none());
        drop(std::fs::remove_dir_all(&dir));
    }
}
