use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::state::context::Kind;

// =============================================================================
// Sidebar Mode
// =============================================================================

/// Controls sidebar display: full, collapsed (icons only), or hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SidebarMode {
    #[default]
    /// Full sidebar with panel names and details.
    Normal,
    /// Icons-only sidebar (narrow).
    Collapsed,
    /// Sidebar completely hidden.
    Hidden,
}

impl SidebarMode {
    /// Cycle to the next mode: Normal → Collapsed → Hidden → Normal
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Normal => Self::Collapsed,
            Self::Collapsed => Self::Hidden,
            Self::Hidden => Self::Normal,
        }
    }

    /// Width in columns for this sidebar mode
    #[must_use]
    pub const fn width(self) -> u16 {
        match self {
            Self::Normal => 36,
            Self::Collapsed => 14,
            Self::Hidden => 0,
        }
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
    /// Sidebar display mode (Normal/Collapsed/Hidden)
    /// Sidebar display mode (Normal/Collapsed/Hidden)
    #[serde(default)]
    pub sidebar_mode: SidebarMode,

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
            active_theme: crate::config::DEFAULT_THEME.to_string(),
            owner_pid: None,
            selected_context: 0,
            draft_input: String::new(),
            draft_cursor: 0,
            sidebar_mode: SidebarMode::default(),
            modules: HashMap::new(),
        }
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
            worker_id: crate::config::constants::DEFAULT_WORKER_ID.to_string(),
            important_panel_uids: HashMap::new(),
            panel_uid_to_local_id: HashMap::new(),
            next_tool_id: 1,
            next_result_id: 1,
            modules: HashMap::new(),
        }
    }
}

/// Panel data stored in panels/{uid}.json
/// All panels are stored here - fixed (System, Conversation, Tree, etc.) and dynamic (File, Glob, Grep, Tmux)
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Returns the default schema version (1) for serde `default` attributes.
const fn default_schema_version() -> u32 {
    1
}

/// Returns the default theme ID string for serde `default` attributes.
fn default_theme() -> String {
    crate::config::DEFAULT_THEME.to_string()
}

/// Returns 1, used as serde `default` for ID counters that start at 1.
const fn default_one() -> usize {
    1
}
