use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::context::ContextType;

// =============================================================================
// MULTI-WORKER STATE STRUCTS
// =============================================================================

/// Shared configuration (config.json)
/// Infrastructure fields + module data under "modules" key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedConfig {
    // === Infrastructure ===
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

    // === Module data (keyed by module ID) ===
    #[serde(default)]
    pub modules: HashMap<String, serde_json::Value>,
}

impl Default for SharedConfig {
    fn default() -> Self {
        Self {
            reload_requested: false,
            active_theme: crate::config::DEFAULT_THEME.to_string(),
            owner_pid: None,
            selected_context: 0,
            draft_input: String::new(),
            draft_cursor: 0,
            modules: HashMap::new(),
        }
    }
}

/// Worker-specific state (states/{worker}.json)
/// Infrastructure fields + module data under "modules" key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerState {
    /// Worker identifier
    pub worker_id: String,

    // === Panel UIDs ===
    /// UIDs of important/fixed panels this worker uses
    #[serde(default)]
    pub important_panel_uids: ImportantPanelUids,
    /// Maps panel UIDs to local display IDs (excluding chat which is in important_panel_uids)
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
    #[serde(default)]
    pub modules: HashMap<String, serde_json::Value>,
}

impl Default for WorkerState {
    fn default() -> Self {
        Self {
            worker_id: crate::constants::DEFAULT_WORKER_ID.to_string(),
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
    pub panel_type: ContextType,
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

    // === File/Glob/Grep/Tmux panel metadata ===
    /// File path (for File context)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// Glob pattern
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob_pattern: Option<String>,
    /// Glob search path
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob_path: Option<String>,
    /// Grep regex pattern
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep_pattern: Option<String>,
    /// Grep search path
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep_path: Option<String>,
    /// Grep file filter pattern
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep_file_pattern: Option<String>,
    /// Tmux pane ID
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_pane_id: Option<String>,
    /// Number of lines to capture from tmux pane
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_lines: Option<usize>,
    /// Tmux pane description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_description: Option<String>,
    /// Command string for GitResult/GithubResult panels
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_command: Option<String>,
    /// SHA-256 hash of result_command (for dedup)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_command_hash: Option<String>,
    /// Skill prompt ID (for Skill panels)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_prompt_id: Option<String>,
    /// Content hash for change detection across reloads
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// Accumulated panel cost in USD (never resets)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel_total_cost: Option<f64>,
}

/// UIDs for important/fixed panels that a worker uses.
/// Maps ContextType to panel UID string.
pub type ImportantPanelUids = HashMap<ContextType, String>;

fn default_theme() -> String {
    crate::config::DEFAULT_THEME.to_string()
}

fn default_one() -> usize {
    1
}
