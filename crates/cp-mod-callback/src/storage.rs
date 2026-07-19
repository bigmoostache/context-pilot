//! YAML-backed persistent storage for callback definitions.
//!
//! Callbacks are stored in `.context-pilot/shared/callbacks.yaml`,
//! keyed by callback **name** (alphabetically sorted by `BTreeMap`).
//! This makes YAML diffs merge-friendly across git branches.
//!
//! Delegates all YAML I/O (load, save, backup, recovery) to
//! [`cp_base::config::yaml_sync::YamlSync`].  This module adds
//! callback-specific logic: script file management, header stripping,
//! population of missing definitions from YAML on branch switch.
//!
//! ## Write path
//!
//! After every `Callback_upsert` create/update/delete, the corresponding
//! YAML entry is upserted or deleted via the synchronizer.
//!
//! ## Read path
//!
//! On module load (or branch switch), callbacks that exist in YAML but
//! are missing from the in-memory state are populated back, with their
//! script files recreated on disk.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use cp_base::config::constants;
use cp_base::config::yaml_sync::{SyncEntry, YamlSync};

use crate::types::{CallbackDefinition, CallbackState};

// ---------------------------------------------------------------------------
// YamlSync instance
// ---------------------------------------------------------------------------

/// Shared YAML path for callback definitions.
const SHARED_YAML: &str = ".context-pilot/shared/callbacks.yaml";

/// Worker-local backup filename.
const BACKUP_NAME: &str = "callbacks.yaml.bak";

/// Create a configured `YamlSync` instance for callback definitions.
fn sync() -> YamlSync {
    YamlSync::new(SHARED_YAML, BACKUP_NAME)
}

// ---------------------------------------------------------------------------
// YAML entry type
// ---------------------------------------------------------------------------

/// A single entry in the YAML file.
///
/// Keyed by callback name in the `BTreeMap`. The `name` field is included
/// for backward compatibility with the old hash-keyed format — it is
/// redundant with the map key in the new format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct YamlCallbackEntry {
    /// Callback display name (redundant with map key; kept for migration).
    #[serde(default)]
    pub name: String,
    /// Short description.
    pub description: String,
    /// Gitignore-style glob pattern.
    pub pattern: String,
    /// Whether this callback blocks Edit/Write tool results.
    pub blocking: bool,
    /// Max execution time in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    /// Custom message shown on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success_message: Option<String>,
    /// Working directory for the script.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Global (true) or local/per-file (false).
    pub is_global: bool,
    /// Inline script content (the body, without the auto-generated header).
    pub script_content: String,
    /// Timestamp for conflict resolution (ms since Unix epoch).
    /// Legacy entries default to `0`; any real timestamp wins.
    #[serde(default)]
    pub last_edited_ms: u64,
}

impl SyncEntry for YamlCallbackEntry {
    fn last_edited_ms(&self) -> u64 {
        self.last_edited_ms
    }

    fn set_last_edited_ms(&mut self, ms: u64) {
        self.last_edited_ms = ms;
    }
}

// ---------------------------------------------------------------------------
// Script file helpers
// ---------------------------------------------------------------------------

/// Read the user-authored script body from a callback's script file.
///
/// Strips the auto-generated header (shebang + set + comment block) to store
/// only the user's code in YAML.
fn read_script_body(name: &str) -> Option<String> {
    let script_path = PathBuf::from(constants::STORE_DIR).join("scripts").join(format!("{name}.sh"));
    let content = fs::read_to_string(&script_path).ok()?;
    let body = strip_script_header(&content);
    Some(body.to_owned())
}

/// Strip the auto-generated bash header, returning only the user's script body.
fn strip_script_header(content: &str) -> &str {
    // The generated header looks like:
    //   #!/usr/bin/env bash
    //   set -euo pipefail
    //
    //   # Callback: ...
    //   # Pattern: ...
    //   # Description: ...
    //   #
    //   # Environment variables ...
    //   #   ...
    //
    //   <user code>
    //
    // Strategy: find the last comment block followed by a blank line.
    let mut last_header_end = 0;
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.starts_with("set ") || trimmed.is_empty() {
            // Still in header region
            last_header_end = content
                .lines()
                .take(i.saturating_add(1))
                .map(|l| l.len().saturating_add(1)) // +1 for newline
                .sum();
        } else {
            break;
        }
    }
    content.get(last_header_end..).unwrap_or("").trim_start_matches('\n')
}

/// Write a callback's script file from inline YAML content.
fn write_script_file(name: &str, pattern: &str, description: &str, script_body: &str) {
    let scripts_dir = PathBuf::from(constants::STORE_DIR).join("scripts");
    let _mkdir = fs::create_dir_all(&scripts_dir);
    let script_path = scripts_dir.join(format!("{name}.sh"));

    let full_script = format!(
        "#!/usr/bin/env bash\n\
         set -euo pipefail\n\
         \n\
         # Callback: {name}\n\
         # Pattern: {pattern}\n\
         # Description: {description}\n\
         #\n\
         # Environment variables provided by Context Pilot:\n\
         #   $CP_CHANGED_FILES  — newline-separated list of changed file paths (relative to project root)\n\
         #   $CP_PROJECT_ROOT   — absolute path to project root\n\
         #   $CP_CALLBACK_NAME  — name of this callback rule\n\
         \n\
         {script_body}",
    );

    let _write = fs::write(&script_path, &full_script);
    let _chmod = fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755));
}

// ---------------------------------------------------------------------------
// Public API — surgical updates
// ---------------------------------------------------------------------------

/// Insert or update a callback entry in the YAML store.
///
/// Reads the script file from disk to store its content inline.
/// Skips built-in callbacks (they are system-managed).
pub(crate) fn upsert_yaml_entry(def: &CallbackDefinition) {
    if def.built_in {
        return;
    }
    let Some(script_body) = read_script_body(&def.name) else { return };

    let mut entry = YamlCallbackEntry {
        name: def.name.clone(),
        description: def.description.clone(),
        pattern: def.pattern.clone(),
        blocking: def.blocking,
        timeout_secs: def.timeout_secs,
        success_message: def.success_message.clone(),
        cwd: def.cwd.clone(),
        is_global: def.is_global,
        script_content: script_body,
        last_edited_ms: 0, // set by YamlSync::upsert
    };
    sync().upsert(&def.name, &mut entry);
}

/// Remove a callback entry from the YAML store by name.
pub(crate) fn remove_yaml_entry(name: &str) {
    sync().remove::<YamlCallbackEntry>(name);
}

// ---------------------------------------------------------------------------
// Public API — bulk population
// ---------------------------------------------------------------------------

/// Populate missing in-memory callbacks from the YAML store.
///
/// For each YAML entry whose name is **not** already present in `state.definitions`,
/// a new `CallbackDefinition` is created, its script file is written to disk,
/// and it is added to the active set (by name).
///
/// IDs are set to placeholder values — call `assign_deterministic_ids()` after
/// this function to get stable CB1/CB2/... IDs.
pub(crate) fn populate_from_yaml(state: &mut CallbackState) {
    let map = sync().load::<YamlCallbackEntry>();
    if map.is_empty() {
        return;
    }

    let existing: HashSet<String> = state.definitions.iter().map(|d| d.name.clone()).collect();

    for entry in map.values() {
        if existing.contains(&entry.name) {
            continue;
        }

        // Write the script file to disk
        write_script_file(&entry.name, &entry.pattern, &entry.description, &entry.script_content);

        // Create the definition (placeholder ID — reassigned by assign_deterministic_ids)
        let def = CallbackDefinition {
            id: String::new(),
            name: entry.name.clone(),
            description: entry.description.clone(),
            pattern: entry.pattern.clone(),
            blocking: entry.blocking,
            timeout_secs: entry.timeout_secs,
            success_message: entry.success_message.clone(),
            cwd: entry.cwd.clone(),
            is_global: entry.is_global,
            built_in: false,
            built_in_command: None,
        };

        state.definitions.push(def);
    }
}

/// Migrate existing in-memory callbacks into the YAML store (first-run).
///
/// Only writes entries that are **not** already present in the YAML
/// (keyed by name).  Skips built-in callbacks.  This is idempotent.
pub(crate) fn migrate_to_yaml(definitions: &[CallbackDefinition]) {
    if definitions.is_empty() {
        return;
    }

    // Build a BTreeMap of entries to migrate
    let mut entries = BTreeMap::new();
    for def in definitions {
        if def.built_in {
            continue;
        }
        let Some(script_body) = read_script_body(&def.name) else { continue };
        let _prev = entries.insert(
            def.name.clone(),
            YamlCallbackEntry {
                name: def.name.clone(),
                description: def.description.clone(),
                pattern: def.pattern.clone(),
                blocking: def.blocking,
                timeout_secs: def.timeout_secs,
                success_message: def.success_message.clone(),
                cwd: def.cwd.clone(),
                is_global: def.is_global,
                script_content: script_body,
                last_edited_ms: 0,
            },
        );
    }

    sync().migrate(&entries);
}

/// Clean up old hash-keyed entries from the YAML file.
///
/// The old format used `SHA-256(name)[0..16]` as `BTreeMap` keys. The new format
/// uses the callback name directly. This function detects old hex-keyed entries,
/// re-keys them by name, and writes the cleaned file back.
///
/// Idempotent — safe to call on every load.
pub(crate) fn cleanup_old_hash_keys() {
    let s = sync();
    let map = s.load::<YamlCallbackEntry>();
    if map.is_empty() {
        return;
    }

    // Detect if any key looks like a hex hash (16+ hex chars, not a plausible callback name)
    let old_entries: Vec<YamlCallbackEntry> = map
        .iter()
        .filter(|(k, _)| k.len() >= 16 && k.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|(_, v)| v.clone())
        .collect();

    if old_entries.is_empty() {
        return;
    }

    // Remove old hex-keyed entries
    let is_hex_key = |k: &str, _: &YamlCallbackEntry| k.len() >= 16 && k.chars().all(|c| c.is_ascii_hexdigit());
    let _removed = s.remove_where::<YamlCallbackEntry, _>(is_hex_key);

    // Re-insert with name-based keys (upsert auto-sets last_edited_ms)
    for mut entry in old_entries {
        if !entry.name.is_empty() {
            let name = entry.name.clone();
            s.upsert(&name, &mut entry);
        }
    }
}
