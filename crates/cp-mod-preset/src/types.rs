use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use cp_base::state::context::Kind;

/// Function pointer that returns all registered modules.
pub type ModuleRegistry = fn() -> Vec<Box<dyn cp_base::modules::Module>>;
/// Function pointer that builds active tool definitions from enabled module IDs.
pub type ToolDefBuilder = fn(&std::collections::HashSet<String>) -> Vec<cp_base::tools::ToolDefinition>;
/// Function pointer that ensures default fixed panels exist for active modules.
pub type DefaultsInitializer = fn(&mut cp_base::state::runtime::State);

/// A named preset that captures a worker's full configuration state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    /// Preset identifier (alphanumeric + hyphens).
    #[serde(rename = "preset_name")]
    pub name: String,
    /// Human-readable description of what this preset is for.
    pub description: String,
    /// Whether this is a built-in (non-deletable) preset.
    pub built_in: bool,
    /// Captured worker configuration.
    pub worker_state: PresetWorkerState,
}

/// The worker configuration captured by a preset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetWorkerState {
    /// Which system prompt ID is active
    pub active_agent_id: Option<String>,
    /// Which modules are active (by module ID)
    pub active_modules: Vec<String>,
    /// Which tools are disabled (by tool ID)
    pub disabled_tools: Vec<String>,
    /// Per-worker module data (keyed by module ID)
    #[serde(default)]
    pub modules: HashMap<String, serde_json::Value>,
    /// Which skill IDs are loaded
    #[serde(default)]
    pub loaded_skill_ids: Vec<String>,
    /// Dynamic panel configurations
    #[serde(default)]
    pub dynamic_panels: Vec<PresetPanelConfig>,
}

/// Configuration for a dynamic panel captured by a preset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetPanelConfig {
    /// Panel type (File, Glob, Grep, Tmux, Skill, etc.).
    pub panel_type: Kind,
    /// Display name for the panel tab.
    pub name: String,
    /// File path (for File panels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// Glob pattern (for Glob panels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob_pattern: Option<String>,
    /// Search directory for glob (for Glob panels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob_path: Option<String>,
    /// Grep regex pattern (for Grep panels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep_pattern: Option<String>,
    /// Search directory for grep (for Grep panels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep_path: Option<String>,
    /// File filter pattern for grep (for Grep panels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep_file_pattern: Option<String>,
    /// Tmux pane ID (for Tmux panels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_pane_id: Option<String>,
    /// Number of lines to capture (for Tmux panels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_lines: Option<usize>,
    /// Description text (for Tmux panels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_description: Option<String>,
    /// Skill prompt ID (for Skill panels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_prompt_id: Option<String>,
}
