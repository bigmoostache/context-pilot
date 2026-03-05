use cp_base::state::runtime::State;

// === Git change types ===

/// Classification of how a file was changed in the working tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitChangeType {
    /// Content modified.
    Modified,
    /// New file staged.
    Added,
    /// Not tracked by git.
    Untracked,
    /// Removed from the working tree.
    Deleted,
    /// Path changed (possibly with content edits).
    Renamed,
}

/// A single file change with diff stats.
#[derive(Debug, Clone)]
pub struct GitFileChange {
    /// Relative file path.
    pub path: String,
    /// Lines added.
    pub additions: i32,
    /// Lines deleted.
    pub deletions: i32,
    /// Type of change.
    pub change_type: GitChangeType,
}

// === Module-owned state ===

/// Live git repository state, refreshed on every cache tick.
#[derive(Debug)]
pub struct GitState {
    /// Current branch name (None if detached HEAD).
    pub branch: Option<String>,
    /// All local branches: (name, `is_current`).
    pub branches: Vec<(String, bool)>,
    /// Whether the project root is inside a git repository.
    pub is_repo: bool,
    /// File-level diff stats against `diff_base`.
    pub file_changes: Vec<GitFileChange>,
    /// Ref used as diff base (e.g., "main", "HEAD~3"). None = default branch.
    pub diff_base: Option<String>,
}

impl Default for GitState {
    fn default() -> Self {
        Self::new()
    }
}

impl GitState {
    /// Create a fresh state with no git info.
    #[must_use]
    pub const fn new() -> Self {
        Self { branch: None, branches: vec![], is_repo: false, file_changes: vec![], diff_base: None }
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

/// Payload for a git result panel cache refresh request.
#[derive(Debug)]
pub struct GitResultRequest {
    /// Context element ID (e.g., "P12").
    pub context_id: String,
    /// Git command to re-run for content refresh.
    pub command: String,
}
