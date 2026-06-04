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
    hex.get(..16).unwrap_or(&hex).to_string()
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

    let mut entry = YamlEntry { path: path.to_string(), description: description.to_string(), last_edited_ms: 0 };
    sync().upsert(&key, &mut entry);
}

/// Remove all YAML entries for a given path.
pub(crate) fn remove_yaml_entry(path: &str) {
    let owned_path = path.to_string();
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
pub(crate) fn refresh_stale_from_yaml(descriptions: &mut [TreeFileDescription]) -> bool {
    let map = sync().load::<YamlEntry>();
    if map.is_empty() {
        return false;
    }

    let mut changed = false;
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
            desc.description.clone_from(&entry.description);
            desc.file_hash = current_hash;
            changed = true;
        }
    }
    changed
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
