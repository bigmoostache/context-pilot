use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::state::ContextType;

/// A named preset that captures a worker's full configuration state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub preset_name: String,
    pub description: String,
    pub built_in: bool,
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

/// Configuration for a dynamic panel (File, Glob, Grep, Tmux).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetPanelConfig {
    pub panel_type: ContextType,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob_pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep_pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep_file_pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_pane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_lines: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_prompt_id: Option<String>,
}
