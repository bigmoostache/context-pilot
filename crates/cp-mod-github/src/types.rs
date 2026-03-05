use cp_base::state::runtime::State;

/// Info about a PR associated with the current branch
#[derive(Debug, Clone)]
pub struct BranchPrInfo {
    /// PR number (e.g., 42).
    pub number: u64,
    /// PR title text.
    pub title: String,
    /// PR state: "OPEN", "CLOSED", "MERGED".
    pub state: String,
    /// Full URL to the PR on GitHub.
    pub url: String,
    /// Total lines added (from diff stats).
    pub additions: Option<u64>,
    /// Total lines removed (from diff stats).
    pub deletions: Option<u64>,
    /// Review decision: "APPROVED", "`CHANGES_REQUESTED`", "`REVIEW_REQUIRED`", etc.
    pub review_decision: Option<String>,
    /// CI checks status: "SUCCESS", "FAILURE", "PENDING", etc.
    pub checks_status: Option<String>,
}

/// Runtime state for the GitHub module.
#[derive(Debug)]
pub struct GithubState {
    /// GitHub personal access token (from `GITHUB_TOKEN` env).
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
    /// Create a fresh state with no token or PR info.
    #[must_use]
    pub const fn new() -> Self {
        Self { github_token: None, branch_pr: None }
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

/// Payload for a GitHub result panel cache refresh request.
#[derive(Debug)]
pub struct GithubResultRequest {
    /// Context element ID (e.g., "P15").
    pub context_id: String,
    /// `gh` CLI command to re-run for content refresh.
    pub command: String,
    /// GitHub token for authentication.
    pub github_token: String,
}
