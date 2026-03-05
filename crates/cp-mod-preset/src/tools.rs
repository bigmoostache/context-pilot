use std::collections::HashSet;
use std::fs;
use std::path::Path;

use cp_base::config::constants;

use crate::PRESETS_DIR;
use crate::types::{DefaultsInitializer, ModuleRegistry, Preset, PresetPanelConfig, PresetWorkerState, ToolDefBuilder};
use cp_base::state::context::{Kind, make_default_entry};
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};
use cp_mod_prompt::types::PromptState;

/// Return the directory path where preset JSON files are stored.
fn presets_path() -> std::path::PathBuf {
    Path::new(constants::STORE_DIR).join(PRESETS_DIR)
}

/// Return the full file path for a named preset JSON file.
fn preset_file_path(name: &str) -> std::path::PathBuf {
    presets_path().join(format!("{name}.json"))
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

/// Execute `preset_snapshot_myself`: capture current worker config as a named preset.
pub(crate) fn execute_snapshot(tool: &ToolUse, state: &State, all_modules: ModuleRegistry) -> ToolResult {
    let Some(name) = tool.input.get("name").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing required 'name' parameter".to_string(), true);
    };

    let Some(description) = tool.input.get("description").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing required 'description' parameter".to_string(), true);
    };

    if let Err(e) = validate_name(name) {
        return ToolResult::new(tool.id.clone(), e, true);
    }

    let replace = tool.input.get("replace").and_then(|v| v.as_str());

    let file_path = preset_file_path(name);

    // Handle replace logic
    if let Some(replace_name) = replace {
        let replace_path = preset_file_path(replace_name);
        if replace_path.exists() {
            // Check if it's a built-in preset
            if let Ok(contents) = fs::read_to_string(&replace_path)
                && let Ok(existing) = serde_json::from_str::<Preset>(&contents)
                && existing.built_in
            {
                return ToolResult::new(
                    tool.id.clone(),
                    format!("Cannot replace built-in preset '{replace_name}'"),
                    true,
                );
            }
            let _ = fs::remove_file(&replace_path).ok();
        }
    } else if file_path.exists() {
        return ToolResult::new(
            tool.id.clone(),
            format!("Preset '{name}' already exists. Use the 'replace' parameter to overwrite it."),
            true,
        );
    }

    // Capture worker state
    let modules = all_modules();

    // Capture active_modules
    let active_modules: Vec<String> = state.active_modules.iter().cloned().collect();

    // Capture disabled_tools
    let disabled_tools: Vec<String> = state.tools.iter().filter(|t| !t.enabled).map(|t| t.id.clone()).collect();

    // Capture per-worker module data (non-global modules only)
    let mut module_data = std::collections::HashMap::new();
    for module in &modules {
        if !module.is_global() {
            let data = module.save_module_data(state);
            if !data.is_null() {
                drop(module_data.insert(module.id().to_string(), data));
            }
        }
    }

    // Capture dynamic panels
    let dynamic_panels: Vec<PresetPanelConfig> = state
        .context
        .iter()
        .filter(|ctx| !ctx.context_type.is_fixed())
        .map(|ctx| PresetPanelConfig {
            panel_type: ctx.context_type.clone(),
            name: ctx.name.clone(),
            file_path: ctx.get_meta_str("file_path").map(ToString::to_string),
            glob_pattern: ctx.get_meta_str("glob_pattern").map(ToString::to_string),
            glob_path: ctx.get_meta_str("glob_path").map(ToString::to_string),
            grep_pattern: ctx.get_meta_str("grep_pattern").map(ToString::to_string),
            grep_path: ctx.get_meta_str("grep_path").map(ToString::to_string),
            grep_file_pattern: ctx.get_meta_str("grep_file_pattern").map(ToString::to_string),
            tmux_pane_id: ctx.get_meta_str("tmux_pane_id").map(ToString::to_string),
            tmux_lines: ctx.get_meta_usize("tmux_lines"),
            tmux_description: ctx.get_meta_str("tmux_description").map(ToString::to_string),
            skill_prompt_id: ctx.get_meta_str("skill_prompt_id").map(ToString::to_string),
        })
        .collect();

    let preset = Preset {
        name: name.to_string(),
        description: description.to_string(),
        built_in: false,
        worker_state: PresetWorkerState {
            active_agent_id: PromptState::get(state).active_agent_id.clone(),
            active_modules,
            disabled_tools,
            loaded_skill_ids: PromptState::get(state).loaded_skill_ids.clone(),
            modules: module_data,
            dynamic_panels,
        },
    };

    // Ensure directory exists
    let dir = presets_path();
    if let Err(e) = fs::create_dir_all(&dir) {
        return ToolResult::new(tool.id.clone(), format!("Failed to create presets directory: {e}"), true);
    }

    // Write preset file
    match serde_json::to_string_pretty(&preset) {
        Ok(json) => {
            if let Err(e) = fs::write(&file_path, json) {
                return ToolResult::new(tool.id.clone(), format!("Failed to write preset file: {e}"), true);
            }
        }
        Err(e) => {
            return ToolResult::new(tool.id.clone(), format!("Failed to serialize preset: {e}"), true);
        }
    }

    let panel_count = preset.worker_state.dynamic_panels.len();
    let module_count = preset.worker_state.active_modules.len();
    ToolResult::new(
        tool.id.clone(),
        format!("Preset '{name}' saved ({module_count} modules, {panel_count} dynamic panels)"),
        false,
    )
}

/// Bundled function pointers for preset load operations.
pub(crate) struct LoadCallbacks {
    /// Returns all registered module implementations.
    pub all_modules: ModuleRegistry,
    /// Builds tool definitions from currently active modules.
    pub active_tool_defs: ToolDefBuilder,
    /// Initializes default state for modules missing persisted data.
    pub ensure_defaults: DefaultsInitializer,
}

/// Execute `preset_load`: restore a saved preset configuration.
pub(crate) fn execute_load(tool: &ToolUse, state: &mut State, cb: &LoadCallbacks) -> ToolResult {
    let Some(name) = tool.input.get("name").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing required 'name' parameter".to_string(), true);
    };

    let file_path = preset_file_path(name);

    if !file_path.exists() {
        // List available presets in error message
        let available = list_available_presets();
        let msg = if available.is_empty() {
            format!("Preset '{name}' not found. No presets available.")
        } else {
            format!("Preset '{}' not found. Available presets: {}", name, available.join(", "))
        };
        return ToolResult::new(tool.id.clone(), msg, true);
    }

    let preset: Preset = match fs::read_to_string(&file_path) {
        Ok(json) => match serde_json::from_str(&json) {
            Ok(p) => p,
            Err(e) => {
                return ToolResult::new(tool.id.clone(), format!("Failed to parse preset '{name}': {e}"), true);
            }
        },
        Err(e) => {
            return ToolResult::new(tool.id.clone(), format!("Failed to read preset '{name}': {e}"), true);
        }
    };

    let ws = &preset.worker_state;

    // 1. Set active_agent_id (only if the referenced system exists)
    if let Some(ref sys_id) = ws.active_agent_id
        && PromptState::get(state).agents.iter().any(|s| s.id == *sys_id)
    {
        PromptState::get_mut(state).active_agent_id = Some(sys_id.clone());
    }
    // If system doesn't exist, keep current active_agent_id

    // 2. Set active_modules — ensure core modules are always included
    let modules = (cb.all_modules)();
    let mut core_ids: Vec<String> = modules.iter().filter(|m| m.is_core()).map(|m| m.id().to_string()).collect();
    core_ids.sort();
    let mut new_active: HashSet<String> = ws.active_modules.iter().cloned().collect();
    // Always include core modules
    for core_id in &core_ids {
        let _ = new_active.insert(core_id.clone());
    }
    // Filter to only known modules
    let known_ids: HashSet<String> = modules.iter().map(|m| m.id().to_string()).collect();
    new_active.retain(|id| known_ids.contains(id));
    state.active_modules = new_active;

    // 3. Rebuild tools from active modules, then apply disabled_tools
    let disabled_set: HashSet<&str> = ws.disabled_tools.iter().map(String::as_str).collect();
    let mut new_tools = (cb.active_tool_defs)(&state.active_modules);
    for t in &mut new_tools {
        if t.id != "tool_manage" && t.id != "module_toggle" && disabled_set.contains(t.id.as_str()) {
            t.enabled = false;
        }
    }
    state.tools = new_tools;

    // 4. Reset per-worker module state to defaults, then load preset data
    for module in &modules {
        if !module.is_global() {
            module.reset_state(state);
        }
    }

    // Load preset module data for non-global modules
    for module in &modules {
        if !module.is_global()
            && let Some(data) = ws.modules.get(module.id())
        {
            module.load_module_data(data, state);
        }
    }

    // 5. Remove existing dynamic panels
    state.context.retain(|ctx| ctx.context_type.is_fixed());

    // 6. Recreate dynamic panels from preset config
    for panel_cfg in &ws.dynamic_panels {
        let context_id = state.next_available_context_id();
        let uid = format!("UID_{}_P", state.global_next_uid);
        state.global_next_uid = state.global_next_uid.saturating_add(1);

        let mut elem = make_default_entry(&context_id, panel_cfg.panel_type.clone(), &panel_cfg.name, true);
        elem.uid = Some(uid);
        if let Some(ref v) = panel_cfg.file_path {
            elem.set_meta("file_path", v);
        }
        if let Some(ref v) = panel_cfg.glob_pattern {
            elem.set_meta("glob_pattern", v);
        }
        if let Some(ref v) = panel_cfg.glob_path {
            elem.set_meta("glob_path", v);
        }
        if let Some(ref v) = panel_cfg.grep_pattern {
            elem.set_meta("grep_pattern", v);
        }
        if let Some(ref v) = panel_cfg.grep_path {
            elem.set_meta("grep_path", v);
        }
        if let Some(ref v) = panel_cfg.grep_file_pattern {
            elem.set_meta("grep_file_pattern", v);
        }
        if let Some(ref v) = panel_cfg.tmux_pane_id {
            elem.set_meta("tmux_pane_id", v);
        }
        if let Some(v) = panel_cfg.tmux_lines {
            elem.set_meta("tmux_lines", &v);
        }
        if let Some(ref v) = panel_cfg.tmux_description {
            elem.set_meta("tmux_description", v);
        }
        if let Some(ref v) = panel_cfg.skill_prompt_id {
            elem.set_meta("skill_prompt_id", v);
        }
        state.context.push(elem);
    }

    // 6b. Restore loaded_skill_ids (filter to skills that still exist)
    {
        let ps = PromptState::get(state);
        let valid_ids: Vec<String> =
            ws.loaded_skill_ids.iter().filter(|id| ps.skills.iter().any(|s| &s.id == *id)).cloned().collect();
        PromptState::get_mut(state).loaded_skill_ids = valid_ids;
    }

    // 6c. Populate cached_content for restored skill panels
    {
        let skill_contents: Vec<(String, String)> =
            PromptState::get(state).skills.iter().map(|s| (s.id.clone(), s.content.clone())).collect();
        for ctx in &mut state.context {
            if ctx.context_type.as_str() == Kind::SKILL
                && let Some(skill_id) = ctx.get_meta_str("skill_prompt_id").map(ToString::to_string)
                && let Some((_, content)) = skill_contents.iter().find(|(id, _)| *id == skill_id)
            {
                ctx.cached_content = Some(content.clone());
            }
        }
    }

    // 7. Ensure default fixed panels exist for newly activated modules
    (cb.ensure_defaults)(state);

    // 8. Mark all panels as cache_deprecated
    for ctx in &mut state.context {
        ctx.cache_deprecated = true;
    }

    let module_count = state.active_modules.len();
    let panel_count = ws.dynamic_panels.len();
    ToolResult::new(
        tool.id.clone(),
        format!(
            "Loaded preset '{}': {} — {} modules, {} dynamic panels restored",
            name, preset.description, module_count, panel_count
        ),
        false,
    )
}

/// List all available preset names
fn list_available_presets() -> Vec<String> {
    let dir = presets_path();
    let mut names = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.path().file_stem().and_then(|s| s.to_str()).map(ToString::to_string) {
                names.push(name);
            }
        }
    }
    names.sort();
    names
}

/// Summary info for a preset, used by the overview panel.
#[derive(Debug)]
pub struct PresetInfo {
    /// Preset name (alphanumeric + hyphens).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Whether this preset ships with the binary.
    pub built_in: bool,
}

/// List all available presets with metadata for display.
#[must_use]
pub fn list_presets_with_info() -> Vec<PresetInfo> {
    let dir = presets_path();
    let mut presets = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(contents) = fs::read_to_string(&path)
                && let Ok(preset) = serde_json::from_str::<Preset>(&contents)
            {
                presets.push(PresetInfo {
                    name: preset.name,
                    description: preset.description,
                    built_in: preset.built_in,
                });
            }
        }
    }
    presets.sort_by(|a, b| a.name.cmp(&b.name));
    presets
}
