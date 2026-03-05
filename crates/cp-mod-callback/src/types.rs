use std::collections::HashSet;

use cp_base::state::runtime::State;
use serde::{Deserialize, Serialize};

/// Serde default helper for `is_global` backward compatibility.
const fn default_true() -> bool {
    true
}

/// A callback rule that fires when matching files are edited.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct CallbackState {
    /// All callback definitions (loaded from global config.json)
    pub definitions: Vec<CallbackDefinition>,
    /// Counter for auto-generating CB IDs
    pub next_id: usize,
    /// Per-worker: which callback IDs are active
    pub active_set: HashSet<String>,
    /// Which callback ID is currently open in the editor (if any)
    pub editor_open: Option<String>,
}

impl Default for CallbackState {
    fn default() -> Self {
        Self::new()
    }
}

impl CallbackState {
    /// Create an empty callback state with ID counter at 1.
    #[must_use]
    pub fn new() -> Self {
        Self { definitions: Vec::new(), next_id: 1, active_set: HashSet::new(), editor_open: None }
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
