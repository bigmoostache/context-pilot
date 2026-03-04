use serde::{Deserialize, Serialize};

use cp_base::state::State;

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
pub const DEFAULT_TREE_FILTER: &str = r#"# Ignore common non-essential directories
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
"#;

/// Module-owned state for the Tree module
#[derive(Debug)]
pub struct TreeState {
    /// Gitignore-style filter patterns controlling which files/folders are shown.
    pub tree_filter: String,
    /// Paths of folders currently open (expanded) in the tree view.
    pub tree_open_folders: Vec<String>,
    /// User-written descriptions attached to files/folders.
    pub tree_descriptions: Vec<TreeFileDescription>,
}

impl Default for TreeState {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeState {
    /// Create a default tree state (root folder open, standard filter).
    pub fn new() -> Self {
        Self {
            tree_filter: DEFAULT_TREE_FILTER.to_string(),
            tree_open_folders: vec![".".to_string()],
            tree_descriptions: vec![],
        }
    }

    /// Get shared ref from State's `TypeMap`.
    pub fn get(state: &State) -> &Self {
        state.get_ext::<Self>().expect("TreeState not initialized")
    }

    /// Get mutable ref from State's `TypeMap`.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.get_ext_mut::<Self>().expect("TreeState not initialized")
    }
}
