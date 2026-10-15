//! YAML-backed persistent storage for tree descriptions.
//!
//! Descriptions are stored in `.context-pilot/shared/tree-descriptions.yaml`,
//! keyed by 16 hex chars of FNV-1a over `(path + file_content)`.  The BTreeMap
//! key order is deterministic, making diffs merge-friendly across git branches.
//!
//! Delegates all YAML I/O (load, save, backup, recovery) to
//! [`cp_base::config::yaml_sync::YamlSync`].  This module adds tree-specific
//! logic: content-hash keys, file-existence checks, stale-description refresh,
//! and pruning of superseded entries.
//!
//! ## Write path
//!
//! After every `tree_describe` add/update/remove, the corresponding YAML
//! entry is upserted or deleted via the synchronizer.
//!
//! ## Read path
//!
//! On module load (or branch switch), descriptions that exist in YAML but
//! are missing from the in-memory state are populated back, provided the
//! file still exists on disk **and** its content hash matches the YAML key
//! (i.e. the description is fresh).
//!
//! ## Pruning
//!
//! Every content change adds a key and nothing removed the old one, so the
//! file grew without bound (~5 entries per path). On load, [`prune_yaml`]
//! keeps per path the entry matching the current content plus the
//! [`KEEP_PER_PATH`] most recent others (enough to restore a description
//! after a branch switch), and drops the rest.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use cp_base::config::yaml_sync::{SyncEntry, YamlSync};

use crate::filehash::{description_key as compute_description_key, digest};
use crate::types::TreeFileDescription;

// ---------------------------------------------------------------------------
// YamlSync instance
// ---------------------------------------------------------------------------

/// Shared YAML path for tree descriptions.
const SHARED_YAML: &str = ".context-pilot/shared/tree-descriptions.yaml";

/// Worker-local backup filename.
const BACKUP_NAME: &str = "tree-descriptions.yaml.bak";

/// Create a configured `YamlSync` instance for tree descriptions.
fn sync() -> YamlSync {
    YamlSync::new(SHARED_YAML, BACKUP_NAME)
}

// ---------------------------------------------------------------------------
// YAML entry type
// ---------------------------------------------------------------------------

/// A single entry in the YAML file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct YamlEntry {
    /// Relative file/folder path (e.g. `src/main.rs`).
    pub path: String,
    /// Human-readable description.
    pub description: String,
    /// Timestamp for conflict resolution (ms since Unix epoch).
    /// Legacy entries default to `0`; any real timestamp wins.
    #[serde(default)]
    pub last_edited_ms: u64,
}

impl SyncEntry for YamlEntry {
    fn last_edited_ms(&self) -> u64 {
        self.last_edited_ms
    }

    fn set_last_edited_ms(&mut self, ms: u64) {
        self.last_edited_ms = ms;
    }
}

// ---------------------------------------------------------------------------
// Public API — surgical updates
// ---------------------------------------------------------------------------

/// Insert or update a single description in the YAML store.
///
/// Reads the file from disk to compute a content-hash key, then delegates
/// to [`YamlSync::upsert`] which auto-sets the `last_edited_ms` timestamp.
pub(crate) fn upsert_yaml_entries(items: &[(&str, &str)]) {
    let entries: Vec<(String, YamlEntry)> = items
        .iter()
        .filter_map(|&(path, description)| {
            let content = std::fs::read(Path::new(path)).ok()?;
            let key = compute_description_key(path, &content);
            Some((key, YamlEntry { path: path.to_owned(), description: description.to_owned(), last_edited_ms: 0 }))
        })
        .collect();
    sync().upsert_many(entries);
}

/// Remove all YAML entries for the given paths (one load, at most one write).
pub(crate) fn remove_yaml_entries(paths: &[String]) {
    if paths.is_empty() {
        return;
    }
    let _removed = sync().remove_where::<YamlEntry, _>(|_key, entry| paths.contains(&entry.path));
}

// ---------------------------------------------------------------------------
// Public API — bulk population
// ---------------------------------------------------------------------------

/// Populate missing in-memory descriptions from the YAML store.
///
/// For each YAML entry whose path:
/// 1. is **not** already described in `descriptions`, AND
/// 2. exists on disk, AND
/// 3. has a content hash matching the YAML key (i.e. description is fresh),
///
/// a new `TreeFileDescription` is appended to the in-memory vec.
pub(crate) fn populate_from_yaml(descriptions: &mut Vec<TreeFileDescription>) {
    let map = sync().load::<YamlEntry>();
    populate_with(&map, descriptions);
}

/// [`populate_from_yaml`] over an already-loaded map (pure apart from file reads).
pub(crate) fn populate_with(map: &BTreeMap<String, YamlEntry>, descriptions: &mut Vec<TreeFileDescription>) {
    let mut existing: std::collections::HashSet<String> = descriptions.iter().map(|d| d.path.clone()).collect();
    for (key, entry) in map {
        if existing.contains(&entry.path) {
            continue;
        }
        // Missing file, or content changed since this entry was written: skip.
        let Some(current) = digest(Path::new(&entry.path)).filter(|d| d.desc_key == *key) else { continue };
        let _new = existing.insert(entry.path.clone());
        descriptions.push(TreeFileDescription {
            path: entry.path.clone(),
            description: entry.description.clone(),
            file_hash: current.file_hash,
        });
    }
}

/// Refresh stale in-memory descriptions from the YAML store.
///
/// For each description where the current file hash no longer matches
/// (i.e. the file content changed — branch switch, external edit), check
/// whether the YAML has an entry keyed by the **current** content.  If so,
/// emit a swap of the description and hash.
///
/// Runs on the cache worker thread over the shared snapshot (read-only) and
/// returns one [`DescPatch`] per swap; callers apply them via [`apply_patches`].
pub(crate) fn refresh_stale_from_yaml(descriptions: &[TreeFileDescription]) -> Vec<DescPatch> {
    refresh_with(&load_cached(), descriptions)
}

/// [`refresh_stale_from_yaml`] over an already-loaded map. One memoized
/// [`digest`] per file: an unchanged file costs a `stat`, not a read.
pub(crate) fn refresh_with(map: &BTreeMap<String, YamlEntry>, descriptions: &[TreeFileDescription]) -> Vec<DescPatch> {
    if map.is_empty() {
        return Vec::new();
    }
    let mut patches = Vec::new();
    for desc in descriptions {
        let Some(current) = digest(Path::new(&desc.path)) else { continue };
        if !desc.file_hash.is_empty() && desc.file_hash == current.file_hash {
            continue;
        }
        if let Some(entry) = map.get(&current.desc_key) {
            patches.push(DescPatch {
                path: desc.path.clone(),
                old_hash: desc.file_hash.clone(),
                description: entry.description.clone(),
                file_hash: current.file_hash,
            });
        }
    }
    patches
}

/// Non-protected entries kept per path by [`prune_yaml`]: enough history to
/// restore a description after switching between a few branches.
pub(crate) const KEEP_PER_PATH: usize = 3;

/// Drop superseded YAML entries (one load, at most one write).
///
/// Protected, never dropped: the entry keyed by each path's current content,
/// and any entry whose `(path, description)` is live in `descriptions`.
pub(crate) fn prune_yaml(descriptions: &[TreeFileDescription]) {
    let map = sync().load::<YamlEntry>();
    let live: std::collections::HashSet<(&str, &str)> =
        descriptions.iter().map(|d| (d.path.as_str(), d.description.as_str())).collect();
    let mut protected = std::collections::HashSet::new();
    let mut seen = std::collections::HashSet::new();
    for (key, entry) in &map {
        if live.contains(&(entry.path.as_str(), entry.description.as_str())) {
            let _new = protected.insert(key.clone());
        }
        if seen.insert(entry.path.as_str())
            && let Some(current) = digest(Path::new(&entry.path))
        {
            let _new = protected.insert(current.desc_key);
        }
    }
    let drop = prune_plan(&map, &protected, KEEP_PER_PATH);
    if !drop.is_empty() {
        let _removed = sync().remove_where::<YamlEntry, _>(|key, _entry| drop.contains(key));
    }
}

/// Prunable entries of one path: `(key, last_edited_ms)`.
type Candidates<'map> = Vec<(&'map String, u64)>;

/// Keys to drop: per path, everything but `protected` keys and the `keep`
/// newest others (by `last_edited_ms`, then key for determinism).
pub(crate) fn prune_plan(
    map: &BTreeMap<String, YamlEntry>,
    protected: &std::collections::HashSet<String>,
    keep: usize,
) -> std::collections::HashSet<String> {
    let mut by_path: BTreeMap<&str, Candidates<'_>> = BTreeMap::new();
    for (key, entry) in map {
        if !protected.contains(key) {
            by_path.entry(entry.path.as_str()).or_default().push((key, entry.last_edited_ms));
        }
    }
    let mut drop = std::collections::HashSet::new();
    for mut candidates in by_path.into_values() {
        candidates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        drop.extend(candidates.into_iter().skip(keep).map(|(key, _ms)| key.clone()));
    }
    drop
}

/// One stale-description swap computed off-thread by [`refresh_stale_from_yaml`].
#[derive(Clone)]
pub(crate) struct DescPatch {
    /// Described path.
    pub path: String,
    /// Hash the description carried when the worker snapshotted it; the patch
    /// is skipped if the live entry moved on (e.g. a racing `tree_describe`).
    pub old_hash: String,
    /// Fresh description text from the YAML.
    pub description: String,
    /// Current content hash of the file.
    pub file_hash: String,
}

/// Apply worker-computed patches to the live descriptions. Returns `true` if
/// any entry changed.
pub(crate) fn apply_patches(descriptions: &mut [TreeFileDescription], patches: Vec<DescPatch>) -> bool {
    let mut changed = false;
    for patch in patches {
        let live = descriptions.iter_mut().find(|d| d.path == patch.path);
        if let Some(desc) = live.filter(|d| d.file_hash == patch.old_hash) {
            desc.description = patch.description;
            desc.file_hash = patch.file_hash;
            changed = true;
        }
    }
    changed
}

/// Parsed YAML plus the `(mtime, len)` signature it was parsed at.
type YamlCache = Option<((Option<std::time::SystemTime>, u64), Arc<BTreeMap<String, YamlEntry>>)>;

/// Process-wide parse cache for the (multi-MB) shared YAML.
static YAML_CACHE: Mutex<YamlCache> = Mutex::new(None);

/// The shared YAML map, re-parsed only when the file's mtime or size changed.
fn load_cached() -> Arc<BTreeMap<String, YamlEntry>> {
    let sig = std::fs::metadata(SHARED_YAML).map_or((None, 0), |m| (m.modified().ok(), m.len()));
    let mut slot = YAML_CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(map) = slot.as_ref().filter(|entry| entry.0 == sig).map(|entry| Arc::clone(&entry.1)) {
        return map;
    }
    let map = Arc::new(sync().load::<YamlEntry>());
    *slot = Some((sig, Arc::clone(&map)));
    map
}

/// Migrate existing in-memory descriptions into the YAML store (first-run).
///
/// Only writes entries that are **not** already present in the YAML
/// (keyed by path).  This is idempotent.
pub(crate) fn migrate_to_yaml(descriptions: &[TreeFileDescription]) {
    if descriptions.is_empty() {
        return;
    }

    // Build a BTreeMap of entries to migrate
    let mut entries = BTreeMap::new();
    for desc in descriptions {
        let file_path = Path::new(&desc.path);
        let Ok(content) = std::fs::read(file_path) else { continue };
        let key = compute_description_key(&desc.path, &content);
        let _prev = entries.insert(
            key,
            YamlEntry { path: desc.path.clone(), description: desc.description.clone(), last_edited_ms: 0 },
        );
    }

    sync().migrate(&entries);
}
