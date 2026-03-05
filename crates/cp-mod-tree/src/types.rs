use serde::{Deserialize, Serialize};

use cp_base::state::runtime::State;

/// A file description in the tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeFileDescription {
    /// Relative file/folder path.
    pub path: String,
    /// Human-readable description shown next to the tree entry.
    pub description: String,
    /// Content hash when description was written (detects stale descriptions via `[!]` marker).
    pub file_hash: String,
}

/// Default tree filter (gitignore-style patterns)
pub const DEFAULT_TREE_FILTER: &str = "# Ignore common non-essential directories
.git/
target/
node_modules/
__pycache__/
.venv/
venv/
dist/
build/
*.pyc
*.pyo
.DS_Store
";

/// Module-owned state for the Tree module
#[derive(Debug)]
pub struct TreeState {
    /// Gitignore-style filter patterns controlling which files/folders are shown.
    pub filter: String,
    /// Paths of folders currently open (expanded) in the tree view.
    pub open_folders: Vec<String>,
    /// User-written descriptions attached to files/folders.
    pub descriptions: Vec<TreeFileDescription>,
}

impl Default for TreeState {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeState {
    /// Create a default tree state (root folder open, standard filter).
    #[must_use]
    pub fn new() -> Self {
        Self { filter: DEFAULT_TREE_FILTER.to_string(), open_folders: vec![".".to_string()], descriptions: vec![] }
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
