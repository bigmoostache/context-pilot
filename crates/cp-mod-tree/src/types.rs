use std::sync::LazyLock;

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

/// The described-file list, shared behind an `Arc`.
///
/// A newtype because `Arc<Vec<_>>` is rejected by `clippy::rc_buffer`, while
/// [`std::sync::Arc::make_mut`] still allows in-place push/retain.
/// Serializes exactly like the inner `Vec`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Descriptions(pub Vec<TreeFileDescription>);

impl std::ops::Deref for Descriptions {
    type Target = Vec<TreeFileDescription>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Descriptions {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
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

/// Fleet-shared module state for the Tree module.
///
/// Holds the gitignore filter and the file/folder descriptions — both
/// genuinely fleet-wide knowledge (descriptions persist to
/// `tree-descriptions.yaml`). The per-agent *view* state (which folders are
/// expanded) lives in [`TreeView`], not here, so one agent expanding folders
/// never bloats another agent's tree.
#[derive(Debug)]
pub struct TreeState {
    /// Gitignore-style filter patterns controlling which files/folders are shown.
    pub filter: String,
    /// User-written descriptions attached to files/folders. Behind an `Arc` so
    /// the per-refresh tree cache request shares it instead of deep-cloning
    /// ~8k entries on the main loop; writers go through [`Arc::make_mut`].
    pub descriptions: std::sync::Arc<Descriptions>,
    /// Change stamp, refreshed on every [`TreeState::get_mut`]. The save path
    /// rewrites `modules/tree.json` only when it moved. **Transient**.
    pub revision: u64,
}

/// Process-wide stamp source: unique across `set_ext` replacements, so a
/// fresh state never reuses a stamp the save path already recorded.
static REVISION_SOURCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl Default for TreeState {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeState {
    /// Create a default tree state (root folder open, standard filter).
    #[must_use]
    pub fn new() -> Self {
        Self {
            filter: DEFAULT_TREE_FILTER.to_owned(),
            descriptions: std::sync::Arc::default(),
            revision: REVISION_SOURCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
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

    /// Get mutable ref from State's `TypeMap`, refreshing the change stamp.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    pub fn get_mut(state: &mut State) -> &mut Self {
        let ts = state.ext_mut::<Self>();
        ts.revision = REVISION_SOURCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ts
    }
}

/// Per-agent tree *view* state: which folders are expanded in this agent's
/// tree panel.
///
/// Carved out of [`TreeState`] (which is fleet-shared) so expanded-folder
/// toggles stay private to the executing agent. Without this split a single
/// shared `open_folders` meant any agent opening files/folders bloated every
/// other agent's tree — including freshly created ones. Stored per-agent via
/// [`State::set_ext_agent`](cp_base::state::runtime::State::set_ext_agent) and
/// persisted through the module's worker-data hooks.
#[derive(Debug)]
pub struct TreeView {
    /// Paths of folders currently open (expanded) in this agent's tree view.
    pub open_folders: Vec<String>,
}

impl Default for TreeView {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeView {
    /// Create a default view (only the root folder expanded).
    #[must_use]
    pub fn new() -> Self {
        Self { open_folders: vec![".".to_owned()] }
    }

    /// Get the per-agent view from `State`, falling back to a shared empty
    /// view when this agent has none yet.
    ///
    /// The view is seeded per-agent by [`crate::TreeModule::init_state`], but a
    /// *global* module only runs `init_state` once (for the boot agent), so
    /// agents created afterwards hold no view until they first toggle a folder.
    /// Readers (file-watcher sync, panel cache request) run against any agent,
    /// so this returns a static empty-folder default instead of panicking.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        /// Shared fallback for agents whose per-agent view was never seeded:
        /// no expanded folders (not even root) so nothing is watched/listed
        /// until the agent explicitly opens one.
        static EMPTY: LazyLock<TreeView> = LazyLock::new(|| TreeView { open_folders: Vec::new() });
        state.get_ext::<Self>().unwrap_or(&EMPTY)
    }

    /// Get a mutable ref to the per-agent view from `State`, seeding a default
    /// view for this agent if it has none yet (see [`Self::get`] for why an
    /// agent may lack one). Self-healing: every mutation path works on any
    /// agent, including freshly created ones.
    pub fn get_mut(state: &mut State) -> &mut Self {
        if state.get_ext::<Self>().is_none() {
            state.set_ext_agent(Self::new());
        }
        state.ext_mut::<Self>()
    }
}
