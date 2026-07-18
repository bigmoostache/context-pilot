//! Boot + hourly filesystem⇄index reconciliation.
//!
//! The live watcher only ever mutates the index for events it sees while
//! running, so anything changed while the agent was down drifts silently
//! (deleted files leave orphan chunks; added/edited files stay stale until
//! touched). This module closes that gap: it snapshots the index's expected
//! filesystem state via a cheap projection, stats the real tree, and queues the
//! delta through the existing indexer command channel.
//!
//! It runs once at boot (subsuming the cold-start full scan — an empty index
//! diffs to "index everything") and again on the hourly tick.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use crate::meili::api::MeiliClient;
use crate::meili::tasks;
use crate::types::{self, IndexerCmd};

/// The cheap `(mtime, size)` fingerprint of a file — both read from a single
/// `fs::metadata()` stat, no content read, no hash. `size` catches
/// timestamp-preserving copies that `mtime` alone would miss.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct FilePrint {
    /// Last-modified time in milliseconds since the Unix epoch.
    pub mtime: u64,
    /// File size in bytes.
    pub size: u64,
}

/// The offline delta: which relative paths to (re)index and which to delete.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ReconcilePlan {
    /// Paths present on disk but missing/stale in the index (offline add/edit).
    pub to_index: Vec<String>,
    /// Paths present in the index but gone from disk (offline delete).
    pub to_delete: Vec<String>,
}

impl ReconcilePlan {
    /// Whether the plan would change anything.
    pub(crate) const fn is_empty(&self) -> bool {
        self.to_index.is_empty() && self.to_delete.is_empty()
    }
}

/// Milliseconds-since-epoch mtime — the SINGLE formula shared with
/// `index_one_file` so the fingerprint stored at index time and the one
/// recomputed at reconcile time agree exactly (otherwise every reconcile would
/// see a spurious "edit" and re-queue the whole tree → infinite churn).
pub(crate) fn mtime_ms(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0_u64, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Pure diff between the index's fingerprints and the disk's fingerprints.
///
/// Deterministic (outputs sorted) so it is trivially unit-testable without a
/// live Meilisearch or filesystem.
pub(crate) fn diff(index: &HashMap<String, FilePrint>, disk: &HashMap<String, FilePrint>) -> ReconcilePlan {
    let mut to_index: Vec<String> = Vec::new();
    let mut to_delete: Vec<String> = Vec::new();

    // In the index but gone from disk → delete (offline delete). Keys collected
    // + sorted so iteration is deterministic (and dodges iter_over_hash_type).
    let mut index_keys: Vec<&String> = index.keys().collect();
    index_keys.sort();
    for path in index_keys {
        if !disk.contains_key(path) {
            to_delete.push(path.clone());
        }
    }

    // On disk but missing or with a changed fingerprint → (re)index.
    let mut disk_keys: Vec<&String> = disk.keys().collect();
    disk_keys.sort();
    for path in disk_keys {
        let dprint = &disk[path];
        match index.get(path) {
            None => to_index.push(path.clone()),                             // offline add
            Some(iprint) if iprint != dprint => to_index.push(path.clone()), // offline edit
            Some(_) => {}                                                    // equal → skip (zero Voyage)
        }
    }

    to_index.sort();
    to_delete.sort();
    ReconcilePlan { to_index, to_delete }
}

/// Snapshot the index's expected filesystem state: one fingerprint per file,
/// deduped by `file_path` (all chunks of a file carry the same fingerprint).
fn index_map(client: &MeiliClient, files_uid: &str) -> Result<HashMap<String, FilePrint>, String> {
    let rows = tasks::fetch_projection(client, files_uid, &["file_path", "last_modified_ms", "size_bytes"])?;
    let mut map: HashMap<String, FilePrint> = HashMap::new();
    for row in rows {
        let Some(path) = row.get("file_path").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let mtime = row.get("last_modified_ms").and_then(serde_json::Value::as_u64).unwrap_or(0);
        let size = row.get("size_bytes").and_then(serde_json::Value::as_u64).unwrap_or(0);
        let _prev = map.insert(path.to_string(), FilePrint { mtime, size });
    }
    Ok(map)
}

/// Stat-walk the tree, collecting a fingerprint for every indexable file (same
/// gate as the live indexer, via [`types::is_indexable`]).
fn disk_map(project_root: &Path) -> HashMap<String, FilePrint> {
    let mut map: HashMap<String, FilePrint> = HashMap::new();
    walk(project_root, project_root, &mut map);
    map
}

/// Recursive helper for [`disk_map`], mirroring the indexer's directory filter.
fn walk(root: &Path, dir: &Path, map: &mut HashMap<String, FilePrint>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_symlink() {
            continue;
        }
        if path.is_dir() {
            let name = entry.file_name();
            if !types::is_excluded_dir(name.to_str().unwrap_or("")) {
                walk(root, &path, map);
            }
        } else if path.is_file() {
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            if !types::is_indexable(&path, root, &meta) {
                continue;
            }
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().to_string();
            let _prev = map.insert(rel, FilePrint { mtime: mtime_ms(&meta), size: meta.len() });
        }
    }
}

/// Compute the offline delta between the files index and the current disk.
///
/// # Errors
///
/// Returns an error if the index projection fetch fails.
pub(crate) fn compute_plan(
    client: &MeiliClient,
    files_uid: &str,
    project_root: &Path,
) -> Result<ReconcilePlan, String> {
    let index = index_map(client, files_uid)?;
    let disk = disk_map(project_root);
    Ok(diff(&index, &disk))
}

/// Queue a plan through the indexer command channel: deletes first (cheap), then
/// (re)indexes. Absolute paths are reconstructed from the project root.
pub(crate) fn send_plan(plan: &ReconcilePlan, project_root: &Path, tx: &Sender<IndexerCmd>) {
    for rel in &plan.to_delete {
        let abs: PathBuf = project_root.join(rel);
        let _r = tx.send(IndexerCmd::DeleteFile(abs));
    }
    for rel in &plan.to_index {
        let abs: PathBuf = project_root.join(rel);
        let _r = tx.send(IndexerCmd::IndexFile(abs));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(mtime: u64, size: u64) -> FilePrint {
        FilePrint { mtime, size }
    }

    #[test]
    fn diff_offline_add() {
        let index = HashMap::new();
        let disk = HashMap::from([("a.rs".to_string(), fp(1, 10))]);
        let plan = diff(&index, &disk);
        assert_eq!(plan.to_index, vec!["a.rs".to_string()]);
        assert!(plan.to_delete.is_empty());
    }

    #[test]
    fn diff_offline_delete() {
        let index = HashMap::from([("a.rs".to_string(), fp(1, 10))]);
        let disk = HashMap::new();
        let plan = diff(&index, &disk);
        assert!(plan.to_index.is_empty());
        assert_eq!(plan.to_delete, vec!["a.rs".to_string()]);
    }

    #[test]
    fn diff_offline_edit_mtime() {
        let index = HashMap::from([("a.rs".to_string(), fp(1, 10))]);
        let disk = HashMap::from([("a.rs".to_string(), fp(2, 10))]);
        assert_eq!(diff(&index, &disk).to_index, vec!["a.rs".to_string()]);
    }

    #[test]
    fn diff_offline_edit_size() {
        let index = HashMap::from([("a.rs".to_string(), fp(1, 10))]);
        let disk = HashMap::from([("a.rs".to_string(), fp(1, 20))]);
        assert_eq!(diff(&index, &disk).to_index, vec!["a.rs".to_string()]);
    }

    #[test]
    fn diff_equal_is_skip() {
        let same = HashMap::from([("a.rs".to_string(), fp(1, 10)), ("b.rs".to_string(), fp(3, 30))]);
        let plan = diff(&same, &same.clone());
        assert!(plan.is_empty(), "identical maps must produce an empty plan");
    }

    #[test]
    fn diff_mixed_is_sorted_and_complete() {
        let index = HashMap::from([
            ("keep.rs".to_string(), fp(1, 1)),
            ("gone.rs".to_string(), fp(1, 1)),
            ("edit.rs".to_string(), fp(1, 1)),
        ]);
        let disk = HashMap::from([
            ("keep.rs".to_string(), fp(1, 1)), // equal
            ("edit.rs".to_string(), fp(9, 1)), // changed
            ("new.rs".to_string(), fp(1, 1)),  // added
        ]);
        let plan = diff(&index, &disk);
        assert_eq!(plan.to_index, vec!["edit.rs".to_string(), "new.rs".to_string()]);
        assert_eq!(plan.to_delete, vec!["gone.rs".to_string()]);
    }
}
