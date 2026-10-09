//! Unified YAML synchronizer engine for cross-branch persistent state.
//!
//! Manages a `BTreeMap<String, E>` in a shared YAML file with:
//! - **Additive merge**: different keys → both sides kept
//! - **Latest-wins**: same key → entry with highest `last_edited_ms` wins
//! - **Corruption recovery**: backup after every successful parse; auto-restore on failure
//!
//! Used by tree descriptions and callback definitions. The alphabetical key
//! order (`BTreeMap`) makes YAML diffs merge-friendly across git branches.
//!
//! ## File layout
//!
//! | Path | Purpose |
//! |------|---------|
//! | `.context-pilot/shared/{name}.yaml` | Shared (version-controlled) backing store |
//! | `.context-pilot/{name}.yaml.bak` | Worker-local backup (last known valid) |

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::cast::Safe as _;

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// A YAML-backed synchronized entry.
///
/// Every entry carries a `last_edited_ms` timestamp for conflict resolution.
/// Legacy entries without a timestamp default to `0` (any real timestamp wins).
pub trait SyncEntry: Serialize + DeserializeOwned + Clone + 'static {
    /// Milliseconds since Unix epoch when this entry was last modified.
    /// Returns `0` for legacy/unknown entries.
    fn last_edited_ms(&self) -> u64;

    /// Update the last-edited timestamp.
    fn set_last_edited_ms(&mut self, ms: u64);
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// A synchronized YAML backing store.
///
/// See [module-level docs](self) for merge strategy and file layout.
#[derive(Debug)]
pub struct YamlSync {
    /// Path to the shared YAML file (relative to project root).
    shared_path: PathBuf,
    /// Path to the worker-local backup (in `.context-pilot/`, not shared).
    backup_path: PathBuf,
}

impl YamlSync {
    /// Create a new synchronizer.
    ///
    /// - `shared_yaml`: relative path to the shared YAML
    ///   (e.g. `.context-pilot/shared/tree-descriptions.yaml`)
    /// - `backup_name`: filename for the backup
    ///   (e.g. `tree-descriptions.yaml.bak`).
    ///   Stored under `.context-pilot/` (worker-local, not shared).
    #[must_use]
    pub fn new(shared_yaml: &str, backup_name: &str) -> Self {
        Self { shared_path: PathBuf::from(shared_yaml), backup_path: PathBuf::from(".context-pilot").join(backup_name) }
    }

    // ----- Load / Save -----

    /// Load the YAML map from disk.
    ///
    /// On success, saves a backup copy.
    /// On parse failure, attempts to restore from backup.
    /// Returns an empty map if both fail.
    #[must_use]
    pub fn load<E>(&self) -> BTreeMap<String, E>
    where
        E: SyncEntry,
    {
        // Try shared YAML
        if let Some(map) = try_parse::<E>(&self.shared_path) {
            self.write_backup(&map);
            return map;
        }

        // Shared YAML is missing or corrupted — try backup
        if let Some(map) = try_parse::<E>(&self.backup_path) {
            // Restore the backup to the shared path
            self.write_yaml(&map);
            return map;
        }

        // Both failed — empty slate
        BTreeMap::new()
    }

    /// Write the full map to the shared YAML file.
    fn write_yaml<E>(&self, map: &BTreeMap<String, E>)
    where
        E: SyncEntry,
    {
        let Some(parent) = self.shared_path.parent() else { return };
        let _mkdir = fs::create_dir_all(parent);
        let Ok(yaml_str) = serde_yaml::to_string(map) else { return };
        let _write = fs::write(&self.shared_path, yaml_str);
    }

    /// Save the map as a worker-local backup.
    fn write_backup<E>(&self, map: &BTreeMap<String, E>)
    where
        E: SyncEntry,
    {
        let Some(parent) = self.backup_path.parent() else { return };
        let _mkdir = fs::create_dir_all(parent);
        let Ok(yaml_str) = serde_yaml::to_string(map) else { return };
        let _write = fs::write(&self.backup_path, yaml_str);
    }

    // ----- Bidirectional sync -----

    /// Bidirectional merge between an in-memory map and the YAML file.
    ///
    /// 1. Loads the YAML map from disk (with backup recovery).
    /// 2. Entries only in YAML → added to `memory`.
    /// 3. Entries only in `memory` → added to YAML.
    /// 4. Entries in both → **latest `last_edited_ms` wins**.
    ///    On tie (both 0 or equal), disk wins.
    /// 5. Writes the merged result back to disk.
    ///
    /// Returns `true` if `memory` was modified.
    pub fn sync<E>(&self, memory: &mut BTreeMap<String, E>) -> bool
    where
        E: SyncEntry,
    {
        let yaml_map = self.load::<E>();
        if yaml_map.is_empty() && memory.is_empty() {
            return false;
        }

        let mut memory_changed = false;
        let mut merged = BTreeMap::new();

        // Collect all keys from both sides
        let all_keys: std::collections::BTreeSet<String> = yaml_map.keys().chain(memory.keys()).cloned().collect();

        for key in &all_keys {
            let in_yaml = yaml_map.get(key);
            let in_memory = memory.get(key);

            match (in_yaml, in_memory) {
                (Some(yaml_entry), None) => {
                    // Only in YAML → add to memory
                    let _mem = memory.insert(key.clone(), yaml_entry.clone());
                    let _mrg = merged.insert(key.clone(), yaml_entry.clone());
                    memory_changed = true;
                }
                (None, Some(mem_entry)) => {
                    // Only in memory → keep in memory, add to YAML
                    let _mrg = merged.insert(key.clone(), mem_entry.clone());
                }
                (Some(yaml_entry), Some(mem_entry)) => {
                    // Both → latest timestamp wins; tie → YAML wins
                    let yaml_ts = yaml_entry.last_edited_ms();
                    let mem_ts = mem_entry.last_edited_ms();
                    if yaml_ts >= mem_ts {
                        if yaml_ts != mem_ts || yaml_ts == 0 {
                            // YAML is newer or both are legacy (0) — YAML wins
                            let _mem = memory.insert(key.clone(), yaml_entry.clone());
                            memory_changed = true;
                        }
                        let _mrg = merged.insert(key.clone(), yaml_entry.clone());
                    } else {
                        // Memory is newer
                        let _mrg = merged.insert(key.clone(), mem_entry.clone());
                    }
                }
                // Impossible: key came from one of the two maps
                (None, None) => {}
            }
        }

        // Write merged result to disk
        self.write_yaml(&merged);
        self.write_backup(&merged);

        memory_changed
    }

    // ----- Surgical operations -----

    /// Insert or update a single entry.
    ///
    /// Automatically sets `last_edited_ms` to the current time.
    pub fn upsert<E>(&self, key: &str, entry: &mut E)
    where
        E: SyncEntry,
    {
        entry.set_last_edited_ms(now_ms());
        self.upsert_many(vec![(key.to_owned(), entry.clone())]);
    }

    /// Insert or update several entries with a single load and a single write.
    ///
    /// Each entry's `last_edited_ms` is set to the current time. A per-entry
    /// [`upsert`](Self::upsert) loop would re-parse and re-serialize the whole
    /// file N times — ~100 ms on the main loop for a large tree-descriptions YAML.
    pub fn upsert_many<E>(&self, entries: Vec<(String, E)>)
    where
        E: SyncEntry,
    {
        if entries.is_empty() {
            return;
        }
        let now = now_ms();
        let mut map = self.load_for_edit::<E>();
        for (key, mut entry) in entries {
            entry.set_last_edited_ms(now);
            let _prev = map.insert(key, entry);
        }
        self.write_both(&map);
    }

    /// Load for a mutation that rewrites both files anyway: skips the
    /// success-path backup write that [`load`](Self::load) performs.
    fn load_for_edit<E>(&self) -> BTreeMap<String, E>
    where
        E: SyncEntry,
    {
        try_parse::<E>(&self.shared_path).unwrap_or_else(|| self.load::<E>())
    }

    /// Serialize once and write the result to both the shared YAML and the backup.
    fn write_both<E>(&self, map: &BTreeMap<String, E>)
    where
        E: SyncEntry,
    {
        let serialized = {
            let _p = crate::perf_span!("yaml_serialize");
            serde_yaml::to_string(map)
        };
        let Ok(yaml_str) = serialized else { return };
        let _p = crate::perf_span!("yaml_write");
        for path in [&self.shared_path, &self.backup_path] {
            if let Some(parent) = path.parent() {
                let _mkdir = fs::create_dir_all(parent);
            }
            let written = {
                let _w = crate::perf_span!("yaml_fs_write");
                fs::write(path, &yaml_str).is_ok()
            };
            if written {
                let _c = crate::perf_span!("yaml_cache_store");
                cache_store(path, map);
            }
        }
    }

    /// Remove an entry by key.
    pub fn remove<E>(&self, key: &str)
    where
        E: SyncEntry,
    {
        let mut map = self.load_for_edit::<E>();
        if map.remove(key).is_some() {
            self.write_both(&map);
        }
    }

    /// Remove all entries matching a predicate.
    ///
    /// Returns the number of entries removed.
    pub fn remove_where<E, F>(&self, predicate: F) -> usize
    where
        E: SyncEntry,
        F: Fn(&str, &E) -> bool,
    {
        let mut map = self.load_for_edit::<E>();
        let before = map.len();
        map.retain(|k, v| !predicate(k, v));
        let removed = before.saturating_sub(map.len());
        if removed > 0 {
            self.write_both(&map);
        }
        removed
    }

    // ----- Migration -----

    /// Migrate: write entries to YAML that are not already present.
    ///
    /// Existing YAML entries are **never** overwritten. This is an
    /// idempotent first-run migration from in-memory to YAML.
    pub fn migrate<E>(&self, entries: &BTreeMap<String, E>)
    where
        E: SyncEntry,
    {
        if entries.is_empty() {
            return;
        }
        let mut map = self.load::<E>();
        let mut changed = false;
        for (key, entry) in entries {
            if !map.contains_key(key) {
                let _prev = map.insert(key.clone(), entry.clone());
                changed = true;
            }
        }
        if changed {
            self.write_yaml(&map);
            self.write_backup(&map);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Size + modification time: a cached parse is reused only while both match.
type Stamp = (u64, SystemTime);

/// Per-file parse cache: stamp at parse/write time + the type-erased map.
type ParseCache = std::collections::HashMap<PathBuf, (Stamp, Box<dyn std::any::Any>)>;

thread_local! {
    /// Last parsed map per file, with the stamp it was parsed/written at.
    /// Parsing the 28k-line tree-descriptions YAML costs ~23 ms per call.
    static PARSED: std::cell::RefCell<ParseCache> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Current size + mtime of `path`, or `None` if unavailable.
fn file_stamp(path: &Path) -> Option<Stamp> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

/// Remember `map` as the parsed content of `path` at its current stamp.
fn cache_store<E>(path: &Path, map: &BTreeMap<String, E>)
where
    E: SyncEntry,
{
    let Some(stamp) = file_stamp(path) else { return };
    PARSED.with(|c| {
        let _prev = c.borrow_mut().insert(path.to_path_buf(), (stamp, Box::new(map.clone())));
    });
}

/// Cached map for `path` if the file is unchanged since it was cached.
fn cache_get<E>(path: &Path) -> Option<BTreeMap<String, E>>
where
    E: SyncEntry,
{
    let stamp = file_stamp(path)?;
    PARSED.with(|c| {
        let cache = c.borrow();
        let entry = cache.get(path)?;
        (entry.0 == stamp).then(|| entry.1.downcast_ref::<BTreeMap<String, E>>().cloned()).flatten()
    })
}

/// Attempt to parse a YAML file. Returns `None` on any failure.
///
/// Served from the per-thread cache while the file's size and mtime are
/// unchanged; any external edit changes the stamp and forces a re-parse.
fn try_parse<E>(path: &Path) -> Option<BTreeMap<String, E>>
where
    E: SyncEntry,
{
    if let Some(map) = {
        let _p = crate::perf_span!("yaml_cache_hit");
        cache_get::<E>(path)
    } {
        return Some(map);
    }
    let contents = {
        let _p = crate::perf_span!("yaml_read");
        fs::read_to_string(path).ok()?
    };
    let map: BTreeMap<String, E> = {
        let _p = crate::perf_span!("yaml_parse");
        serde_yaml::from_str(&contents).ok()?
    };
    cache_store(path, &map);
    Some(map)
}

/// Current time in milliseconds since Unix epoch.
fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().to_u64())
}
