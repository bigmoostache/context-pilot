use serde::{Deserialize, Serialize};

/// A file description in the tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeFileDescription {
    /// File path (relative to project root)
    pub path: String,
    /// Description of the file
    pub description: String,
    /// File hash when description was created (to detect staleness)
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
