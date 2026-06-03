//! YAML-backed persistent storage for memory items.
//!
//! Memories are stored in `.context-pilot/shared/memories.yaml`,
//! keyed by a stable `yaml_key` (SHA-256 of content at creation time).
//! This makes YAML diffs merge-friendly across git branches.
//!
//! Delegates all YAML I/O to [`cp_base::config::yaml_sync::YamlSync`].
//!
//! ## Write path
//!
//! After every `memory_create` or `memory_update`, the corresponding
//! YAML entry is upserted or removed via the synchronizer.
//!
//! ## Read path
//!
//! On module load (or branch switch), memories that exist in YAML but
//! are missing from the in-memory state are populated back.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use cp_base::config::yaml_sync::{SyncEntry, YamlSync};

use crate::types::{MemoryImportance, MemoryItem, MemoryState};

// ---------------------------------------------------------------------------
// YamlSync instance
// ---------------------------------------------------------------------------

/// Shared YAML path for memories.
const SHARED_YAML: &str = ".context-pilot/shared/memories.yaml";

/// Worker-local backup filename.
const BACKUP_NAME: &str = "memories.yaml.bak";

/// Create a configured `YamlSync` instance for memories.
fn sync() -> YamlSync {
    YamlSync::new(SHARED_YAML, BACKUP_NAME)
}

// ---------------------------------------------------------------------------
// YAML entry type
// ---------------------------------------------------------------------------

/// A single entry in the YAML file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct YamlMemoryEntry {
    /// Short summary (tl;dr).
    pub tl_dr: String,
    /// Rich body text (shown when memory is opened).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub contents: String,
    /// Importance level.
    #[serde(default)]
    pub importance: MemoryImportance,
    /// Freeform labels.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// Timestamp for conflict resolution (ms since Unix epoch).
    #[serde(default)]
    pub last_edited_ms: u64,
}

impl SyncEntry for YamlMemoryEntry {
    fn last_edited_ms(&self) -> u64 {
        self.last_edited_ms
    }

    fn set_last_edited_ms(&mut self, ms: u64) {
        self.last_edited_ms = ms;
    }
}

// ---------------------------------------------------------------------------
// Key generation
// ---------------------------------------------------------------------------

/// Generate a stable YAML key for a memory item.
///
/// Uses `SHA-256(tl_dr)[0..16]` — content-addressed at creation time.
/// The key is stored on the `MemoryItem` and never changes on update.
#[must_use]
pub(crate) fn generate_yaml_key(tl_dr: &str) -> String {
    let hash = Sha256::digest(tl_dr.as_bytes());
    let hex = format!("{hash:x}");
    hex.get(..16).unwrap_or(&hex).to_string()
}

/// Ensure a memory item has a `yaml_key`, generating one if missing.
///
/// Called during migration for pre-existing memories that lack a key.
pub(crate) fn ensure_yaml_key(item: &mut MemoryItem) {
    if item.yaml_key.is_empty() {
        item.yaml_key = generate_yaml_key(&item.tl_dr);
    }
}

// ---------------------------------------------------------------------------
// Public API — surgical updates
// ---------------------------------------------------------------------------

/// Insert or update a memory entry in the YAML store.
pub(crate) fn upsert_yaml_entry(item: &MemoryItem) {
    if item.yaml_key.is_empty() {
        return;
    }
    let mut entry = YamlMemoryEntry {
        tl_dr: item.tl_dr.clone(),
        contents: item.contents.clone(),
        importance: item.importance,
        labels: item.labels.clone(),
        last_edited_ms: 0, // set by YamlSync::upsert
    };
    sync().upsert(&item.yaml_key, &mut entry);
}

/// Remove a memory entry from the YAML store by key.
pub(crate) fn remove_yaml_entry(yaml_key: &str) {
    if yaml_key.is_empty() {
        return;
    }
    sync().remove::<YamlMemoryEntry>(yaml_key);
}

// ---------------------------------------------------------------------------
// Public API — bulk population
// ---------------------------------------------------------------------------

/// Populate missing in-memory memories from the YAML store.
///
/// For each YAML entry whose key is **not** already present in any
/// `MemoryItem.yaml_key`, a new memory is created and added to state.
pub(crate) fn populate_from_yaml(state: &mut MemoryState) {
    let map = sync().load::<YamlMemoryEntry>();
    if map.is_empty() {
        return;
    }

    let existing_keys: std::collections::HashSet<String> = state.memories.iter().map(|m| m.yaml_key.clone()).collect();

    for (key, entry) in &map {
        if existing_keys.contains(key) {
            continue;
        }

        let id = format!("M{}", state.next_memory_id);
        state.next_memory_id = state.next_memory_id.saturating_add(1);

        state.memories.push(MemoryItem {
            id,
            tl_dr: entry.tl_dr.clone(),
            contents: entry.contents.clone(),
            importance: entry.importance,
            labels: entry.labels.clone(),
            yaml_key: key.clone(),
        });
    }
}

/// Migrate existing in-memory memories into the YAML store (first-run).
///
/// Ensures all memories have `yaml_keys`, then writes entries that are
/// **not** already present in the YAML. Idempotent.
pub(crate) fn migrate_to_yaml(memories: &mut [MemoryItem]) {
    if memories.is_empty() {
        return;
    }

    // Ensure all items have yaml_keys
    for item in memories.iter_mut() {
        ensure_yaml_key(item);
    }

    // Build a BTreeMap of entries to migrate
    let mut entries = BTreeMap::new();
    for item in memories.iter() {
        let _prev = entries.insert(
            item.yaml_key.clone(),
            YamlMemoryEntry {
                tl_dr: item.tl_dr.clone(),
                contents: item.contents.clone(),
                importance: item.importance,
                labels: item.labels.clone(),
                last_edited_ms: 0,
            },
        );
    }

    sync().migrate(&entries);
}
