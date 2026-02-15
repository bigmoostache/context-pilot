/// Represents a file change in git status
#[derive(Debug, Clone)]
pub struct GitFileChange {
    /// File path (relative to repo root)
    pub path: String,
    /// Lines added
    pub additions: i32,
    /// Lines deleted
    pub deletions: i32,
    /// Type of change
    pub change_type: GitChangeType,
    /// Diff content for this file (unified diff format)
    pub diff_content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitChangeType {
    /// Modified file (staged or unstaged)
    Modified,
    /// Newly added file (staged)
    Added,
    /// Untracked file (not in git)
    Untracked,
    /// Deleted file
    Deleted,
    /// Renamed file
    Renamed,
}
