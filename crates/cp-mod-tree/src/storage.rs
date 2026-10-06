//! YAML-backed persistent storage for tree descriptions.
//!
//! Descriptions are stored in `.context-pilot/shared/tree-descriptions.yaml`,
//! keyed by a 16-char hex SHA-256 of `(path + file_content)`.  The BTreeMap
//! key order is deterministic, making diffs merge-friendly across git branches.
//!
//! Delegates all YAML I/O (load, save, backup, recovery) to
//! [`cp_base::config::yaml_sync::YamlSync`].  This module adds tree-specific
//! logic: content-hash keys, file-existence checks, stale-description refresh.
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

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use cp_base::config::yaml_sync::{SyncEntry, YamlSync};

use crate::tools::compute_file_hash;
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
// Key computation
// ---------------------------------------------------------------------------

/// Compute a description key: first 16 hex chars of FNV-1a(path ∥ content).
///
/// The combined hash means the same file at a different path (copy/rename)
/// or the same path with different content each get their own YAML entry.
fn compute_description_key(path: &str, content: &[u8]) -> String {
    let mut data = Vec::with_capacity(path.len().saturating_add(content.len()));
    data.extend_from_slice(path.as_bytes());
    data.extend_from_slice(content);
    let hex = cp_mod_utilities::hash::compute(&data);
    hex.get(..16).unwrap_or(&hex).to_owned()
}

// ---------------------------------------------------------------------------
// Public API — surgical updates
// ---------------------------------------------------------------------------

/// Insert or update a single description in the YAML store.
///
/// Reads the file from disk to compute a content-hash key, then delegates
/// to [`YamlSync::upsert`] which auto-sets the `last_edited_ms` timestamp.
pub(crate) fn upsert_yaml_entry(path: &str, description: &str) {
    let file_path = Path::new(path);
    let Ok(content) = std::fs::read(file_path) else { return };
    let key = compute_description_key(path, &content);

    let mut entry = YamlEntry { path: path.to_owned(), description: description.to_owned(), last_edited_ms: 0 };
    sync().upsert(&key, &mut entry);
}

/// Remove all YAML entries for a given path.
pub(crate) fn remove_yaml_entry(path: &str) {
    let owned_path = path.to_owned();
    let _removed = sync().remove_where::<YamlEntry, _>(|_key, entry| entry.path == owned_path);
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
    if map.is_empty() {
        return;
    }

    let existing: std::collections::HashSet<String> = descriptions.iter().map(|d| d.path.clone()).collect();

    for (key, entry) in &map {
        if existing.contains(&entry.path) {
            continue;
        }
        let file_path = Path::new(&entry.path);
        if !file_path.exists() {
            continue;
        }
        let Ok(content) = std::fs::read(file_path) else { continue };
        let current_key = compute_description_key(&entry.path, &content);
        if current_key != *key {
            continue; // File content changed — description is stale, skip
        }
        let file_hash = compute_file_hash(file_path).unwrap_or_default();
        descriptions.push(TreeFileDescription {
            path: entry.path.clone(),
            description: entry.description.clone(),
            file_hash,
        });
    }
}

/// Refresh stale in-memory descriptions from the YAML store.
///
/// For each description where the current file hash no longer matches
/// (i.e. the file content changed — branch switch, external edit), check
/// whether the YAML has an entry keyed by the **current** content.  If so,
/// swap the description and hash in-place.  Returns `true` if anything changed.
///
/// Runs on the cache worker thread: mutates the worker's copy and returns one
/// [`DescPatch`] per swap so the main thread can replay them cheaply.
pub(crate) fn refresh_stale_from_yaml(descriptions: &mut [TreeFileDescription]) -> Vec<DescPatch> {
    let map = load_cached();
    if map.is_empty() {
        return Vec::new();
    }

    let mut patches = Vec::new();
    for desc in descriptions.iter_mut() {
        let file_path = Path::new(&desc.path);
        let Some(current_hash) = compute_file_hash(file_path) else { continue };

        // Skip if description is still fresh
        if !desc.file_hash.is_empty() && desc.file_hash == current_hash {
            continue;
        }

        // File content changed — check YAML for a matching entry
        let Ok(content) = std::fs::read(file_path) else { continue };
        let current_key = compute_description_key(&desc.path, &content);

        if let Some(entry) = map.get(&current_key) {
            let old_hash = std::mem::replace(&mut desc.file_hash, current_hash);
            desc.description.clone_from(&entry.description);
            patches.push(DescPatch {
                path: desc.path.clone(),
                old_hash,
                description: desc.description.clone(),
                file_hash: desc.file_hash.clone(),
            });
        }
    }
    patches
}

/// One stale-description swap computed off-thread by [`refresh_stale_from_yaml`].
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
