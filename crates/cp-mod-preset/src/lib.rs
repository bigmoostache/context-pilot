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

use std::collections::HashSet;

use cp_base::modules::{Module, ToolVisualizer};
use cp_base::panels::Panel;
use cp_base::state::{ContextType, State};
use cp_base::tools::{ParamType, PreFlightResult, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/preset.yaml")));

/// Injected callbacks for module-registry operations that live in the binary.
/// The crate doesn't depend on the binary — these function pointers bridge the gap.
#[derive(Debug, Clone, Copy)]
pub struct PresetModule {
    pub(crate) all_modules: fn() -> Vec<Box<dyn Module>>,
    pub(crate) active_tool_defs: fn(&HashSet<String>) -> Vec<ToolDefinition>,
    pub(crate) ensure_defaults: fn(&mut State),
}

impl PresetModule {
    /// Create a new `PresetModule` with injected function pointers for module registry access.
    pub fn new(
        all_modules: fn() -> Vec<Box<dyn Module>>,
        active_tool_defs: fn(&HashSet<String>) -> Vec<ToolDefinition>,
        ensure_defaults: fn(&mut State),
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

    fn is_core(&self) -> bool {
        true
    }
    fn is_global(&self) -> bool {
        true
    }

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

    fn pre_flight(&self, tool: &ToolUse, _state: &State) -> Option<PreFlightResult> {
        match tool.name.as_str() {
            "preset_load" => {
                let mut pf = PreFlightResult::new();
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
                let mut pf = PreFlightResult::new();
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
            "preset_load" => {
                Some(tools::execute_load(tool, state, self.all_modules, self.active_tool_defs, self.ensure_defaults))
            }
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![("preset_snapshot_myself", visualize_preset_output), ("preset_load", visualize_preset_output)]
    }

    fn create_panel(&self, _context_type: &ContextType) -> Option<Box<dyn Panel>> {
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
            output.push_str(&format!("| {} | {} | {} |\n", p.name, ptype, p.description));
        }
        Some(output)
    }
}

/// Visualizer for preset tool results.
/// Shows preset name and lists captured modules/tools with colored indicators.
fn visualize_preset_output(content: &str, width: usize) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::prelude::*;

    let success_color = Color::Rgb(80, 250, 123);
    let info_color = Color::Rgb(139, 233, 253);
    let error_color = Color::Rgb(255, 85, 85);

    let mut lines = Vec::new();

    for line in content.lines() {
        if line.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        let style = if line.starts_with("Error:") {
            Style::default().fg(error_color)
        } else if line.starts_with("Snapshot saved:") || line.starts_with("Loaded preset") {
            Style::default().fg(success_color)
        } else if line.contains("'") {
            // Preset names in quotes
            Style::default().fg(info_color)
        } else {
            Style::default()
        };

        let display = if line.len() > width {
            format!("{}...", &line.get(..line.floor_char_boundary(width.saturating_sub(3))).unwrap_or(""))
        } else {
            line.to_string()
        };
        lines.push(Line::from(Span::styled(display, style)));
    }

    lines
}
