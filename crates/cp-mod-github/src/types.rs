use cp_base::state::State;

/// Info about a PR associated with the current branch
#[derive(Debug, Clone)]
pub struct BranchPrInfo {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub url: String,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub review_decision: Option<String>,
    pub checks_status: Option<String>,
}

#[derive(Debug)]
pub struct GithubState {
    pub github_token: Option<String>,
    /// PR info for the current git branch (if any)
    pub branch_pr: Option<BranchPrInfo>,
}

impl Default for GithubState {
    fn default() -> Self {
        Self::new()
    }
}

impl GithubState {
    pub fn new() -> Self {
        Self { github_token: None, branch_pr: None }
    }
    pub fn get(state: &State) -> &Self {
        state.get_ext::<Self>().expect("GithubState not initialized")
    }
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.get_ext_mut::<Self>().expect("GithubState not initialized")
    }
}

/// Data for CacheRequest when refreshing GitHub result panels
#[derive(Debug)]
pub struct GithubResultRequest {
    pub context_id: String,
    pub command: String,
    pub github_token: String,
}
