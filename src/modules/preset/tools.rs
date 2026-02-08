use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::constants::{STORE_DIR, PRESETS_DIR};
use crate::core::ensure_default_contexts;
use crate::state::{ContextElement, State};
use crate::tools::{ToolResult, ToolUse};

use super::types::{Preset, PresetPanelConfig, PresetWorkerState};

fn presets_path() -> std::path::PathBuf {
    Path::new(STORE_DIR).join(PRESETS_DIR)
}

fn preset_file_path(name: &str) -> std::path::PathBuf {
    presets_path().join(format!("{}.json", name))
}

/// Validate preset name: alphanumeric and hyphens only
fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Preset name cannot be empty".to_string());
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-') {
        return Err("Preset name must contain only alphanumeric characters and hyphens".to_string());
    }
    Ok(())
}

pub fn execute_snapshot(tool: &ToolUse, state: &mut State) -> ToolResult {
    let name = match tool.input.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required 'name' parameter".to_string(),
                is_error: true,
            };
        }
    };

    let description = match tool.input.get("description").and_then(|v| v.as_str()) {
        Some(d) => d,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required 'description' parameter".to_string(),
                is_error: true,
            };
        }
    };

    if let Err(e) = validate_name(name) {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: e,
            is_error: true,
        };
    }

    let replace = tool.input.get("replace").and_then(|v| v.as_str());

    let file_path = preset_file_path(name);

    // Handle replace logic
    if let Some(replace_name) = replace {
        let replace_path = preset_file_path(replace_name);
        if replace_path.exists() {
            // Check if it's a built-in preset
            if let Ok(contents) = fs::read_to_string(&replace_path) {
                if let Ok(existing) = serde_json::from_str::<Preset>(&contents) {
                    if existing.built_in {
                        return ToolResult {
                            tool_use_id: tool.id.clone(),
                            content: format!("Cannot replace built-in preset '{}'", replace_name),
                            is_error: true,
                        };
                    }
                }
            }
            let _ = fs::remove_file(&replace_path);
        }
    } else if file_path.exists() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!(
                "Preset '{}' already exists. Use the 'replace' parameter to overwrite it.",
                name
            ),
            is_error: true,
        };
    }

    // Capture worker state
    let modules = crate::modules::all_modules();

    // Capture active_modules
    let active_modules: Vec<String> = state.active_modules.iter().cloned().collect();

    // Capture disabled_tools
    let disabled_tools: Vec<String> = state
        .tools
        .iter()
        .filter(|t| !t.enabled)
        .map(|t| t.id.clone())
        .collect();

    // Capture per-worker module data (non-global modules only)
    let mut module_data = std::collections::HashMap::new();
    for module in &modules {
        if !module.is_global() {
            let data = module.save_module_data(state);
            if !data.is_null() {
                module_data.insert(module.id().to_string(), data);
            }
        }
    }

    // Capture dynamic panels
    let dynamic_panels: Vec<PresetPanelConfig> = state
        .context
        .iter()
        .filter(|ctx| !ctx.context_type.is_fixed())
        .map(|ctx| PresetPanelConfig {
            panel_type: ctx.context_type,
            name: ctx.name.clone(),
            file_path: ctx.file_path.clone(),
            glob_pattern: ctx.glob_pattern.clone(),
            glob_path: ctx.glob_path.clone(),
            grep_pattern: ctx.grep_pattern.clone(),
            grep_path: ctx.grep_path.clone(),
            grep_file_pattern: ctx.grep_file_pattern.clone(),
            tmux_pane_id: ctx.tmux_pane_id.clone(),
            tmux_lines: ctx.tmux_lines,
            tmux_description: ctx.tmux_description.clone(),
        })
        .collect();

    let preset = Preset {
        preset_name: name.to_string(),
        description: description.to_string(),
        built_in: false,
        worker_state: PresetWorkerState {
            active_system_id: state.active_system_id.clone(),
            active_modules,
            disabled_tools,
            modules: module_data,
            dynamic_panels,
        },
    };

    // Ensure directory exists
    let dir = presets_path();
    if let Err(e) = fs::create_dir_all(&dir) {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Failed to create presets directory: {}", e),
            is_error: true,
        };
    }

    // Write preset file
    match serde_json::to_string_pretty(&preset) {
        Ok(json) => {
            if let Err(e) = fs::write(&file_path, json) {
                return ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Failed to write preset file: {}", e),
                    is_error: true,
                };
            }
        }
        Err(e) => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Failed to serialize preset: {}", e),
                is_error: true,
            };
        }
    }

    let panel_count = preset.worker_state.dynamic_panels.len();
    let module_count = preset.worker_state.active_modules.len();
    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!(
            "Preset '{}' saved ({} modules, {} dynamic panels)",
            name, module_count, panel_count
        ),
        is_error: false,
    }
}

pub fn execute_load(tool: &ToolUse, state: &mut State) -> ToolResult {
    let name = match tool.input.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required 'name' parameter".to_string(),
                is_error: true,
            };
        }
    };

    let file_path = preset_file_path(name);

    if !file_path.exists() {
        // List available presets in error message
        let available = list_available_presets();
        let msg = if available.is_empty() {
            format!("Preset '{}' not found. No presets available.", name)
        } else {
            format!(
                "Preset '{}' not found. Available presets: {}",
                name,
                available.join(", ")
            )
        };
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: msg,
            is_error: true,
        };
    }

    let preset: Preset = match fs::read_to_string(&file_path) {
        Ok(json) => match serde_json::from_str(&json) {
            Ok(p) => p,
            Err(e) => {
                return ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Failed to parse preset '{}': {}", name, e),
                    is_error: true,
                };
            }
        },
        Err(e) => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Failed to read preset '{}': {}", name, e),
                is_error: true,
            };
        }
    };

    let ws = &preset.worker_state;

    // 1. Set active_system_id (only if the referenced system exists)
    if let Some(ref sys_id) = ws.active_system_id {
        if state.systems.iter().any(|s| s.id == *sys_id) {
            state.active_system_id = Some(sys_id.clone());
        }
        // If system doesn't exist, keep current active_system_id
    }

    // 2. Set active_modules — ensure core modules are always included
    let modules = crate::modules::all_modules();
    let core_ids: HashSet<String> = modules
        .iter()
        .filter(|m| m.is_core())
        .map(|m| m.id().to_string())
        .collect();
    let mut new_active: HashSet<String> = ws.active_modules.iter().cloned().collect();
    // Always include core modules
    for core_id in &core_ids {
        new_active.insert(core_id.clone());
    }
    // Filter to only known modules
    let known_ids: HashSet<String> = modules.iter().map(|m| m.id().to_string()).collect();
    new_active.retain(|id| known_ids.contains(id));
    state.active_modules = new_active;

    // 3. Rebuild tools from active modules, then apply disabled_tools
    let disabled_set: HashSet<&str> = ws.disabled_tools.iter().map(|s| s.as_str()).collect();
    let mut new_tools = crate::modules::active_tool_definitions(&state.active_modules);
    for t in &mut new_tools {
        if t.id != "tool_manage" && t.id != "module_toggle" && disabled_set.contains(t.id.as_str())
        {
            t.enabled = false;
        }
    }
    state.tools = new_tools;

    // 4. Reset per-worker module state to defaults, then load preset data
    // Reset todo state
    state.todos.clear();
    state.next_todo_id = 1;
    // Reset scratchpad state
    state.scratchpad_cells.clear();
    state.next_scratchpad_id = 1;
    // Reset git per-worker settings
    state.git_show_diffs = true;
    state.git_show_logs = false;
    state.git_log_args = None;
    state.git_log_content = None;

    // Load preset module data for non-global modules
    for module in &modules {
        if !module.is_global() {
            if let Some(data) = ws.modules.get(module.id()) {
                module.load_module_data(data, state);
            }
        }
    }

    // 5. Remove existing dynamic panels (kill tmux panes first)
    for ctx in &state.context {
        if ctx.context_type == crate::state::ContextType::Tmux {
            if let Some(pane_id) = &ctx.tmux_pane_id {
                let _ = std::process::Command::new("tmux")
                    .args(["kill-window", "-t", pane_id])
                    .output();
            }
        }
    }
    state
        .context
        .retain(|ctx| ctx.context_type.is_fixed());

    // 6. Recreate dynamic panels from preset config
    for panel_cfg in &ws.dynamic_panels {
        let context_id = state.next_available_context_id();
        let uid = format!("UID_{}_P", state.global_next_uid);
        state.global_next_uid += 1;

        state.context.push(ContextElement {
            id: context_id,
            uid: Some(uid),
            context_type: panel_cfg.panel_type,
            name: panel_cfg.name.clone(),
            token_count: 0,
            file_path: panel_cfg.file_path.clone(),
            file_hash: None,
            glob_pattern: panel_cfg.glob_pattern.clone(),
            glob_path: panel_cfg.glob_path.clone(),
            grep_pattern: panel_cfg.grep_pattern.clone(),
            grep_path: panel_cfg.grep_path.clone(),
            grep_file_pattern: panel_cfg.grep_file_pattern.clone(),
            tmux_pane_id: panel_cfg.tmux_pane_id.clone(),
            tmux_lines: panel_cfg.tmux_lines,
            tmux_last_keys: None,
            tmux_description: panel_cfg.tmux_description.clone(),
            result_command: None,
            result_command_hash: None,
            cached_content: None,
            cache_deprecated: true,
            last_refresh_ms: crate::core::panels::now_ms(),
            tmux_last_lines_hash: None,
            current_page: 0,
            total_pages: 1,
            full_token_count: 0,
        });
    }

    // 7. Ensure default fixed panels exist for newly activated modules
    ensure_default_contexts(state);

    // 8. Mark all panels as cache_deprecated
    for ctx in &mut state.context {
        ctx.cache_deprecated = true;
    }

    let module_count = state.active_modules.len();
    let panel_count = ws.dynamic_panels.len();
    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!(
            "Loaded preset '{}': {} — {} modules, {} dynamic panels restored",
            name, preset.description, module_count, panel_count
        ),
        is_error: false,
    }
}

/// List all available preset names
fn list_available_presets() -> Vec<String> {
    let dir = presets_path();
    let mut names = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry
                .path()
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
            {
                names.push(name);
            }
        }
    }
    names.sort();
    names
}

/// Summary info for a preset, used by the overview panel.
pub struct PresetInfo {
    pub name: String,
    pub description: String,
    pub built_in: bool,
}

/// List all available presets with metadata for display.
pub fn list_presets_with_info() -> Vec<PresetInfo> {
    let dir = presets_path();
    let mut presets = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(contents) = fs::read_to_string(&path) {
                if let Ok(preset) = serde_json::from_str::<Preset>(&contents) {
                    presets.push(PresetInfo {
                        name: preset.preset_name,
                        description: preset.description,
                        built_in: preset.built_in,
                    });
                }
            }
        }
    }
    presets.sort_by(|a, b| a.name.cmp(&b.name));
    presets
}
