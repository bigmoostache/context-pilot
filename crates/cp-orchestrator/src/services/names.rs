//! Orchestrator-owned **agent display-name overrides** (T328).
//!
//! By default an agent's display name is the basename of its realm folder
//! (e.g. `context-pilot` for `/Users/gui/context-pilot`).  This store lets the
//! dashboard user set a custom label per agent, persisted independently of the
//! agent's self-written registry record.
//!
//! # File shape
//!
//! `<agents_dir>/agent-names.json` — a flat JSON object mapping agent id to
//! display name (`{ "<id>": "My Custom Name", ... }`).  Writes are atomic
//! (`tmp` → `rename`).  A missing or unreadable file yields an empty map —
//! every agent falls back to its folder-derived name.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// In-memory + on-disk map of agent id → custom display name.
#[derive(Debug, Default)]
pub struct NameOverrides {
    /// Agent id → display name.
    names: HashMap<String, String>,
    /// The backing file (`<agents_dir>/agent-names.json`).
    path: PathBuf,
}

impl NameOverrides {
    /// Load from `<agents_dir>/agent-names.json`, or start empty.
    ///
    /// A missing or corrupt file silently yields an empty map — naming is a
    /// convenience, never a hard dependency for the backend to boot.
    #[must_use]
    pub fn load(agents_dir: &Path) -> Self {
        let path = agents_dir.join("agent-names.json");
        let names = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<HashMap<String, String>>(&bytes).ok())
            .unwrap_or_default();
        Self { names, path }
    }

    /// Look up a custom display name for `agent_id`.
    ///
    /// Returns `None` when the agent has no override (callers fall back to the
    /// folder-derived basename).
    #[must_use]
    pub fn get(&self, agent_id: &str) -> Option<&str> {
        self.names.get(agent_id).map(String::as_str)
    }

    /// Set or clear a display-name override.
    ///
    /// An empty or whitespace-only `name` **removes** the override (reverts to
    /// the folder-derived default).  Returns the previous override, if any.
    pub fn set(&mut self, agent_id: &str, name: &str) -> Option<String> {
        let trimmed = name.trim();
        let prev = if trimmed.is_empty() {
            self.names.remove(agent_id)
        } else {
            self.names.insert(agent_id.to_owned(), trimmed.to_owned())
        };
        self.persist();
        prev
    }

    /// Atomically write the map to disk (`tmp` → `rename`).
    ///
    /// A write failure is logged and swallowed: the in-memory state stays
    /// authoritative; the worst case is a name override lost across a backend
    /// restart.
    fn persist(&self) {
        let Ok(bytes) = serde_json::to_vec_pretty(&self.names) else {
            eprintln!("names: serialize failed");
            return;
        };
        let tmp = self.path.with_extension("json.tmp");
        if std::fs::write(&tmp, &bytes).is_err() {
            eprintln!("names: write tmp failed: {}", tmp.display());
            return;
        }
        if let Err(e) = std::fs::rename(&tmp, &self.path) {
            eprintln!("names: rename failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_roundtrip() {
        let dir = std::env::temp_dir().join(format!("cp-names-test-{}", std::process::id()));
        drop(std::fs::create_dir_all(&dir));

        let mut store = NameOverrides::load(&dir);
        assert!(store.get("a").is_none());

        let _prev = store.set("a", "My Agent");
        assert_eq!(store.get("a"), Some("My Agent"));

        // Reload from disk proves persistence.
        let reloaded = NameOverrides::load(&dir);
        assert_eq!(reloaded.get("a"), Some("My Agent"));

        // Empty name clears the override.
        let _prev = store.set("a", "  ");
        assert!(store.get("a").is_none());
        assert!(NameOverrides::load(&dir).get("a").is_none());

        drop(std::fs::remove_dir_all(&dir));
    }
}
