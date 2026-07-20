use std::collections::HashMap;

use cp_base::state::runtime::State;
use serde::{Deserialize, Serialize};

/// Serde default helper for `is_global` backward compatibility.
const fn default_true() -> bool {
    true
}

/// A callback rule that fires when matching files are edited.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CallbackDefinition {
    /// Auto-generated ID: "CB1", "CB2", ...
    pub id: String,
    /// User-chosen display name (e.g., "rust-check")
    pub name: String,
    /// Short explanation of what this callback does
    pub description: String,
    /// Gitignore-style glob pattern (e.g., "*.rs", "src/**/*.ts")
    pub pattern: String,
    /// Whether this callback blocks Edit/Write tool results
    pub blocking: bool,
    /// Max execution time in seconds (required for blocking, optional for non-blocking)
    pub timeout_secs: Option<u64>,
    /// Custom message shown on success (e.g., "Build passed ✓")
    pub success_message: Option<String>,
    /// Working directory for the script (defaults to project root)
    pub cwd: Option<String>,
    /// Global callbacks fire once per batch with `$CP_CHANGED_FILES` (plural).
    /// Local callbacks fire once per changed file with `$CP_CHANGED_FILE` (singular).
    #[serde(default = "default_true")]
    pub is_global: bool,
    /// If true, this is a built-in callback (not user-created, no external script).
    /// The command is stored in `built_in_command` and executed directly.
    #[serde(default)]
    pub built_in: bool,
    /// Command to execute for built-in callbacks (e.g., "/path/to/tui typst-compile $FILE").
    /// Each matched file is appended as a separate invocation.
    #[serde(default)]
    pub built_in_command: Option<String>,
}

/// Module-owned state for the Callback module.
/// Stored in `State.module_data` via `TypeMap`.
#[derive(Debug)]
#[non_exhaustive]
pub struct CallbackState {
    /// All callback definitions (loaded from YAML backing store).
    pub definitions: Vec<CallbackDefinition>,
    /// Which callback **name** is currently open in the editor (if any).
    pub editor_open: Option<String>,
    /// Active callback sessions: `callback_id` → `session_key`.
    /// Used for dedup: if the same callback fires again, the old session is killed first.
    /// Ephemeral — not persisted across restarts.
    pub active_sessions: HashMap<String, String>,
}

impl Default for CallbackState {
    fn default() -> Self {
        Self::new()
    }
}

impl CallbackState {
    /// Create an empty callback state.
    #[must_use]
    pub fn new() -> Self {
        Self { definitions: Vec::new(), editor_open: None, active_sessions: HashMap::new() }
    }

    /// Assign deterministic IDs (CB1, CB2, ...) based on alphabetical order of names.
    ///
    /// Called after all definitions are loaded. Makes IDs reproducible across
    /// machines/workers — the same set of callbacks always gets the same IDs.
    pub fn assign_deterministic_ids(&mut self) {
        self.definitions.sort_by(|a, b| a.name.cmp(&b.name));
        for (i, def) in self.definitions.iter_mut().enumerate() {
            def.id = format!("CB{}", i.saturating_add(1));
        }
    }

    /// Look up a callback by name or CB ID.
    ///
    /// Accepts either the callback name (e.g., "rust-clippy") or the ephemeral
    /// CB ID (e.g., "CB5"). Name lookup takes priority.
    #[must_use]
    pub fn find_by_name_or_id(&self, key: &str) -> Option<&CallbackDefinition> {
        self.definitions.iter().find(|d| d.name == key).or_else(|| self.definitions.iter().find(|d| d.id == key))
    }

    /// Find the position of a callback by name or CB ID.
    #[must_use]
    pub fn position_by_name_or_id(&self, key: &str) -> Option<usize> {
        self.definitions
            .iter()
            .position(|d| d.name == key)
            .or_else(|| self.definitions.iter().position(|d| d.id == key))
    }

    /// Resolve a name-or-ID key to the callback's canonical name.
    #[must_use]
    pub fn resolve_name(&self, key: &str) -> Option<String> {
        self.find_by_name_or_id(key).map(|d| d.name.clone())
    }

    /// Get shared ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Get mutable ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }
}
