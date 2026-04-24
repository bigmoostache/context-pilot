//! Preset module — save and load named worker configuration snapshots.
//!
//! Two tools: `preset_snapshot_myself` (capture current config) and
//! `preset_load` (restore a saved config). Built-in presets ship with the
//! binary; custom presets are stored as JSON in `.context-pilot/presets/`.

/// Built-in preset definitions (admin, worker, planner, etc.).
pub mod builtin;
/// Tool implementations: `execute_snapshot`, `execute_load`, preset listing.
pub mod tools;
/// Serde types: `PresetData`, `PresetPanelConfig`, `PresetInfo`.
pub mod types;

/// Presets subdirectory
pub const PRESETS_DIR: &str = "presets";

use cp_base::modules::{Module, ToolVisualizer};
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ParamType, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

/// Lazily parsed tool texts from the preset YAML definition file.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/preset.yaml")));

use crate::types::{DefaultsInitializer, ModuleRegistry, ToolDefBuilder};
use std::fmt::Write as _;

/// Injected callbacks for module-registry operations that live in the binary.
/// The crate doesn't depend on the binary — these function pointers bridge the gap.
#[derive(Debug, Clone, Copy)]
pub struct PresetModule {
    /// Returns the full list of registered [`Module`](cp_base::modules::Module) implementations.
    pub all_modules: ModuleRegistry,
    /// Builds the active tool definition list from currently enabled modules.
    pub active_tool_defs: ToolDefBuilder,
    /// Initializes default module state for any modules lacking persisted data.
    pub ensure_defaults: DefaultsInitializer,
}

impl PresetModule {
    /// Create a new `PresetModule` with injected function pointers for module registry access.
    pub fn new(
        all_modules: ModuleRegistry,
        active_tool_defs: ToolDefBuilder,
        ensure_defaults: DefaultsInitializer,
    ) -> Self {
        Self { all_modules, active_tool_defs, ensure_defaults }
    }
}

impl Module for PresetModule {
    fn id(&self) -> &'static str {
        "preset"
    }
    fn name(&self) -> &'static str {
        "Preset"
    }
    fn description(&self) -> &'static str {
        "Save and load named worker configuration presets"
    }

    fn dependencies(&self) -> &[&'static str] {
        &[]
    }

    fn is_core(&self) -> bool {
        true
    }
    fn is_global(&self) -> bool {
        true
    }

    fn init_state(&self, _state: &mut State) {}

    fn reset_state(&self, _state: &mut State) {}

    fn save_module_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_module_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("preset_snapshot_myself", t)
                .short_desc("Save current config")
                .category("System")
                .param("name", ParamType::String, true)
                .param("description", ParamType::String, true)
                .param("replace", ParamType::String, false)
                .build(),
            ToolDefinition::from_yaml("preset_load", t)
                .short_desc("Load saved config")
                .category("System")
                .param("name", ParamType::String, true)
                .build(),
        ]
    }

    fn pre_flight(&self, tool: &ToolUse, _state: &State) -> Option<Verdict> {
        match tool.name.as_str() {
            "preset_load" => {
                let mut pf = Verdict::new();
                if let Some(name) = tool.input.get("name").and_then(|v| v.as_str()) {
                    let presets = tools::list_presets_with_info();
                    if !presets.iter().any(|p| p.name == name) {
                        let available: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
                        pf.errors.push(format!("Preset '{}' not found. Available: {}", name, available.join(", ")));
                    }
                }
                Some(pf)
            }
            "preset_snapshot_myself" => {
                let mut pf = Verdict::new();
                pf.activate_queue = true;
                if let Some(name) = tool.input.get("name").and_then(|v| v.as_str()) {
                    let replace = tool.input.get("replace").and_then(|v| v.as_str());
                    let presets = tools::list_presets_with_info();
                    if presets.iter().any(|p| p.name == name) && replace.is_none() {
                        pf.errors.push(format!("Preset '{name}' already exists. Pass replace:'{name}' to overwrite."));
                    }
                }
                Some(pf)
            }
            _ => None,
        }
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "preset_snapshot_myself" => Some(tools::execute_snapshot(tool, state, self.all_modules)),
            "preset_load" => Some(tools::execute_load(
                tool,
                state,
                &tools::LoadCallbacks {
                    all_modules: self.all_modules,
                    active_tool_defs: self.active_tool_defs,
                    ensure_defaults: self.ensure_defaults,
                },
            )),
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![("preset_snapshot_myself", visualize_preset_output), ("preset_load", visualize_preset_output)]
    }

    fn create_panel(&self, _context_type: &Kind) -> Option<Box<dyn Panel>> {
        None
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![]
    }

    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
        None
    }

    fn context_detail(&self, _ctx: &cp_base::state::context::Entry) -> Option<String> {
        None
    }

    fn overview_context_section(&self, _state: &State) -> Option<String> {
        let presets = tools::list_presets_with_info();
        if presets.is_empty() {
            return None;
        }
        let mut output = String::from("\nPresets:\n\n");
        output.push_str("| Name | Type | Description |\n");
        output.push_str("|------|------|-------------|\n");
        for p in &presets {
            let ptype = if p.built_in { "built-in" } else { "custom" };
            let _r = writeln!(output, "| {} | {} | {} |", p.name, ptype, p.description);
        }
        Some(output)
    }

    fn overview_render_sections(&self, _state: &State) -> Vec<(u8, Vec<cp_render::Block>)> {
        vec![]
    }

    fn on_close_context(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _state: &mut State,
    ) -> Option<Result<String, String>> {
        None
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![]
    }

    fn on_user_message(&self, _state: &mut State) {}

    fn on_stream_stop(&self, _state: &mut State) {}

    fn on_tool_progress(&self, _tool_name: &str, _input_so_far: &str, _state: &mut State) {}

    fn on_tool_complete(&self, _tool_name: &str, _state: &mut State) {}

    fn watch_paths(&self, _state: &State) -> Vec<cp_base::panels::WatchSpec> {
        vec![]
    }

    fn should_invalidate_on_fs_change(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _changed_path: &str,
        _is_dir_event: bool,
    ) -> bool {
        false
    }

    fn watcher_immediate_refresh(&self) -> bool {
        true
    }
}

/// Visualizer for preset tool results.
/// Shows preset name and lists captured modules/tools with colored indicators.
fn visualize_preset_output(content: &str, width: usize) -> Vec<cp_render::Block> {
    use cp_render::{Block, Semantic, Span};

    content
        .lines()
        .map(|line| {
            if line.is_empty() {
                return Block::empty();
            }
            let semantic = if line.starts_with("Error:") {
                Semantic::Error
            } else if line.starts_with("Snapshot saved:") || line.starts_with("Loaded preset") {
                Semantic::Success
            } else if line.contains('\'') {
                Semantic::Info
            } else {
                Semantic::Default
            };
            let display = if line.len() > width {
                format!("{}...", line.get(..line.floor_char_boundary(width.saturating_sub(3))).unwrap_or(""))
            } else {
                line.to_string()
            };
            Block::Line(vec![Span::styled(display, semantic)])
        })
        .collect()
}
