use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::state::context::Kind;

// =============================================================================
// Sidebar Mode
// =============================================================================

/// Controls the current view mode.
///
/// `Normal` shows the standard panel view (sidebar + content panel).
/// `Threads` replaces the entire layout with a dedicated threads view.
/// Ctrl+V toggles between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ViewMode {
    /// Threads view: dedicated layout for thread management (no panels).
    Threads,
    #[default]
    #[serde(other)]
    /// Full sidebar with panel names and details.
    Normal,
}

impl ViewMode {
    /// Toggle between Normal and Threads.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Normal => Self::Threads,
            Self::Threads => Self::Normal,
        }
    }

    /// Width in columns for the sidebar in this mode.
    /// Returns 0 for Threads (sidebar is not rendered).
    #[must_use]
    pub const fn width(self) -> u16 {
        match self {
            Self::Normal => 36,
            Self::Threads => 0,
        }
    }

    /// Whether this mode shows the standard panel view (sidebar + content panel).
    #[must_use]
    pub const fn is_panel_view(self) -> bool {
        matches!(self, Self::Normal)
    }
}

// =============================================================================
// MULTI-WORKER STATE STRUCTS
// =============================================================================

/// Current schema version for `Shared` config and `WorkerState`.
/// Increment when making breaking changes to the persistence format.
pub const SCHEMA_VERSION: u32 = 1;

/// Shared configuration (`config.json`)
/// Infrastructure fields + module data under "modules" key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shared {
    // === Infrastructure ===
    /// Schema version for forward/backward compatibility
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// Flag to request reload (checked by run.sh supervisor)
    #[serde(default)]
    pub reload_requested: bool,
    /// Active theme ID
    #[serde(default = "default_theme")]
    pub active_theme: String,
    /// PID of the process that owns this state
    #[serde(default)]
    pub owner_pid: Option<u32>,
    /// Selected context index
    #[serde(default)]
    pub selected_context: usize,
    /// Draft input text (not yet sent)
    #[serde(default)]
    pub draft_input: String,
    /// Cursor position in draft input
    #[serde(default)]
    pub draft_cursor: usize,
    /// View mode (Normal/Threads)
    #[serde(default, alias = "sidebar_mode")]
    pub view_mode: ViewMode,

    // === Module data (keyed by module ID) ===
    /// Per-module persistent data, keyed by module ID string.
    #[serde(default)]
    pub modules: HashMap<String, serde_json::Value>,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            reload_requested: false,
            active_theme: crate::config::DEFAULT_THEME.to_owned(),
            owner_pid: None,
            selected_context: 0,
            draft_input: String::new(),
            draft_cursor: 0,
            view_mode: ViewMode::default(),
            modules: HashMap::new(),
        }
    }
}

impl Shared {
    /// Set the selected-panel index and draft input state (builder over `default`).
    #[must_use]
    pub fn with_ui(mut self, selected_context: usize, draft_input: String, draft_cursor: usize) -> Self {
        self.selected_context = selected_context;
        self.draft_input = draft_input;
        self.draft_cursor = draft_cursor;
        self
    }

    /// Set the active theme ID (builder).
    #[must_use]
    pub fn with_active_theme(mut self, theme: String) -> Self {
        self.active_theme = theme;
        self
    }

    /// Set the owning process PID (builder).
    #[must_use]
    pub const fn with_owner_pid(mut self, pid: Option<u32>) -> Self {
        self.owner_pid = pid;
        self
    }

    /// Set the view mode (builder).
    #[must_use]
    pub const fn with_view_mode(mut self, view_mode: ViewMode) -> Self {
        self.view_mode = view_mode;
        self
    }

    /// Set the per-module persistent data map (builder).
    #[must_use]
    pub fn with_modules(mut self, modules: HashMap<String, serde_json::Value>) -> Self {
        self.modules = modules;
        self
    }
}

/// Worker-specific state (states/{worker}.json)
/// Infrastructure fields + module data under "modules" key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerState {
    /// Schema version for forward/backward compatibility
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// Worker identifier
    pub worker_id: String,

    // === Panel UIDs ===
    /// UIDs of important/fixed panels this worker uses
    #[serde(default)]
    pub important_panel_uids: ImportantPanelUids,
    /// Maps panel UIDs to local display IDs (excluding chat which is in `important_panel_uids`)
    #[serde(default)]
    pub panel_uid_to_local_id: HashMap<String, String>,

    // === Local ID counters ===
    /// Next tool message ID
    #[serde(default = "default_one")]
    pub next_tool_id: usize,
    /// Next result message ID
    #[serde(default = "default_one")]
    pub next_result_id: usize,

    // === Module data (keyed by module ID) ===
    /// Per-module persistent worker data, keyed by module ID string.
    #[serde(default)]
    pub modules: HashMap<String, serde_json::Value>,
}

impl Default for WorkerState {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            worker_id: crate::config::constants::DEFAULT_WORKER_ID.to_owned(),
            important_panel_uids: HashMap::new(),
            panel_uid_to_local_id: HashMap::new(),
            next_tool_id: 1,
            next_result_id: 1,
            modules: HashMap::new(),
        }
    }
}

impl WorkerState {
    /// Set the worker identifier (builder over `default`).
    #[must_use]
    pub fn with_worker_id(mut self, worker_id: String) -> Self {
        self.worker_id = worker_id;
        self
    }

    /// Set the important + local-id panel UID maps (builder).
    #[must_use]
    pub fn with_panel_uids(
        mut self,
        important: ImportantPanelUids,
        panel_uid_to_local_id: HashMap<String, String>,
    ) -> Self {
        self.important_panel_uids = important;
        self.panel_uid_to_local_id = panel_uid_to_local_id;
        self
    }

    /// Set the next tool + result message-ID counters (builder).
    #[must_use]
    pub const fn with_id_counters(mut self, next_tool_id: usize, next_result_id: usize) -> Self {
        self.next_tool_id = next_tool_id;
        self.next_result_id = next_result_id;
        self
    }

    /// Set the per-module persistent worker-data map (builder).
    #[must_use]
    pub fn with_modules(mut self, modules: HashMap<String, serde_json::Value>) -> Self {
        self.modules = modules;
        self
    }
}

/// Panel data stored in panels/{uid}.json
/// All panels are stored here - fixed (System, Conversation, Tree, etc.) and dynamic (File, Glob, Grep, Tmux)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PanelData {
    /// UID of this panel
    pub uid: String,
    /// Panel type
    pub panel_type: Kind,
    /// Display name
    pub name: String,
    /// Token count (preserved across sessions)
    #[serde(default)]
    pub token_count: usize,
    /// Last refresh timestamp in milliseconds (preserved across sessions)
    #[serde(default)]
    pub last_refresh_ms: u64,

    // === Conversation panel data ===
    /// Message UIDs for conversation panels
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub message_uids: Vec<String>,

    // === Generic metadata bag for module-specific panel data ===
    /// Keys are module-defined strings (e.g., "`file_path`", "`tmux_pane_id`").
    /// Replaces former hardcoded Option<> fields per module.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,

    /// Content hash for change detection across reloads
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// Accumulated panel cost in USD (never resets)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel_total_cost: Option<f64>,
    /// Total lifetime freezes (persisted across reloads)
    #[serde(default)]
    pub total_freezes: u64,
    /// Total lifetime cache misses (persisted across reloads)
    #[serde(default)]
    pub total_cache_misses: u64,
}

/// UIDs for important/fixed panels that a worker uses.
/// Maps `Kind` to panel UID string.
pub type ImportantPanelUids = HashMap<Kind, String>;

impl PanelData {
    /// A panel-data record from its identity triple; all other fields default —
    /// fill them via the builder setters.
    #[must_use]
    pub fn new(uid: String, panel_type: Kind, name: String) -> Self {
        Self { uid, panel_type, name, ..Self::default() }
    }

    /// Set the token count and last-refresh timestamp (builder).
    #[must_use]
    pub const fn with_metrics(mut self, token_count: usize, last_refresh_ms: u64) -> Self {
        self.token_count = token_count;
        self.last_refresh_ms = last_refresh_ms;
        self
    }

    /// Set the conversation/history message UIDs (builder).
    #[must_use]
    pub fn with_message_uids(mut self, message_uids: Vec<String>) -> Self {
        self.message_uids = message_uids;
        self
    }

    /// Set the metadata bag and content hash (builder).
    #[must_use]
    pub fn with_metadata(mut self, metadata: HashMap<String, serde_json::Value>, content_hash: Option<String>) -> Self {
        self.metadata = metadata;
        self.content_hash = content_hash;
        self
    }

    /// Set the accumulated cost + lifetime freeze/cache-miss counters (builder).
    #[must_use]
    pub const fn with_stats(
        mut self,
        panel_total_cost: Option<f64>,
        total_freezes: u64,
        total_cache_misses: u64,
    ) -> Self {
        self.panel_total_cost = panel_total_cost;
        self.total_freezes = total_freezes;
        self.total_cache_misses = total_cache_misses;
        self
    }
}

/// Returns the default schema version (1) for serde `default` attributes.
const fn default_schema_version() -> u32 {
    1
}

/// Returns the default theme ID string for serde `default` attributes.
fn default_theme() -> String {
    crate::config::DEFAULT_THEME.to_owned()
}

/// Returns 1, used as serde `default` for ID counters that start at 1.
const fn default_one() -> usize {
    1
}
