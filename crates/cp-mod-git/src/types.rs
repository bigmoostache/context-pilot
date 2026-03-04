use cp_base::state::State;

// === Git change types ===

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitChangeType {
    Modified,
    Added,
    Untracked,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone)]
pub struct GitFileChange {
    pub path: String,
    pub additions: i32,
    pub deletions: i32,
    pub change_type: GitChangeType,
}

// === Module-owned state ===

#[derive(Debug)]
pub struct GitState {
    pub git_branch: Option<String>,
    pub git_branches: Vec<(String, bool)>,
    pub git_is_repo: bool,
    pub git_file_changes: Vec<GitFileChange>,
    pub git_diff_base: Option<String>,
}

impl Default for GitState {
    fn default() -> Self {
        Self::new()
    }
}

impl GitState {
    pub fn new() -> Self {
        Self {
            git_branch: None,
            git_branches: vec![],
            git_is_repo: false,
            git_file_changes: vec![],
            git_diff_base: None,
        }
    }
    pub fn get(state: &State) -> &Self {
        state.get_ext::<Self>().expect("GitState not initialized")
    }
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.get_ext_mut::<Self>().expect("GitState not initialized")
    }
}

/// Data for CacheRequest when refreshing git result panels
#[derive(Debug)]
pub struct GitResultRequest {
    pub context_id: String,
    pub command: String,
}
