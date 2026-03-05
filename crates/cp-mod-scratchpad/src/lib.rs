//! Scratchpad module — temporary cells for notes, code snippets, and working data.
//!
//! Three tools: `scratchpad_create_cell`, `scratchpad_edit_cell`, `scratchpad_wipe`.
//! Cells are stored per-worker and shown in a fixed panel. Useful for the AI to
//! maintain intermediate state during multi-step tasks.

mod panel;
mod tools;
/// Scratchpad state types: `ScratchpadCell`, `ScratchpadState`.
pub mod types;

pub use types::{ScratchpadCell, ScratchpadState};

use serde_json::json;

use cp_base::modules::ToolVisualizer;
use cp_base::panels::Panel;
use cp_base::state::{ContextType, State};
use cp_base::tools::{ParamType, PreFlightResult, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::ScratchpadPanel;
use cp_base::cast::SafeCast;
use cp_base::modules::Module;

static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/scratchpad.yaml")));

/// Scratchpad module: temporary note cells for working data during a session.
#[derive(Debug, Clone, Copy)]
pub struct ScratchpadModule;

impl Module for ScratchpadModule {
    fn id(&self) -> &'static str {
        "scratchpad"
    }
    fn name(&self) -> &'static str {
        "Scratchpad"
    }
    fn description(&self) -> &'static str {
        "Temporary note-taking scratchpad cells"
    }

    fn init_state(&self, state: &mut State) {
        state.set_ext(ScratchpadState::new());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(ScratchpadState::new());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let ss = ScratchpadState::get(state);
        json!({
            "scratchpad_cells": ss.scratchpad_cells,
            "next_scratchpad_id": ss.next_scratchpad_id,
        })
    }
    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        let ss = ScratchpadState::get_mut(state);
        if let Some(arr) = data.get("scratchpad_cells")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            ss.scratchpad_cells = v;
        }
        if let Some(v) = data.get("next_scratchpad_id").and_then(serde_json::Value::as_u64) {
            ss.next_scratchpad_id = v.to_usize();
        }
    }

    fn fixed_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::new(ContextType::SCRATCHPAD)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> {
        vec![(ContextType::new(ContextType::SCRATCHPAD), "Scratch", false)]
    }

    fn create_panel(&self, context_type: &ContextType) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            ContextType::SCRATCHPAD => Some(Box::new(ScratchpadPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("scratchpad_create_cell", t)
                .short_desc("Add scratchpad cell")
                .category("Scratchpad")
                .reverie_allowed(true)
                .param("cell_title", ParamType::String, true)
                .param("cell_contents", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("scratchpad_edit_cell", t)
                .short_desc("Modify scratchpad cell")
                .category("Scratchpad")
                .reverie_allowed(true)
                .param("cell_id", ParamType::String, true)
                .param("cell_title", ParamType::String, false)
                .param("cell_contents", ParamType::String, false)
                .build(),
            ToolDefinition::from_yaml("scratchpad_wipe", t)
                .short_desc("Delete scratchpad cells")
                .category("Scratchpad")
                .reverie_allowed(true)
                .param_array("cell_ids", ParamType::String, true)
                .build(),
        ]
    }

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<PreFlightResult> {
        match tool.name.as_str() {
            "scratchpad_edit_cell" => {
                let mut pf = PreFlightResult::new();
                if let Some(cell_id) = tool.input.get("cell_id").and_then(|v| v.as_str()) {
                    let ss = ScratchpadState::get(state);
                    if !ss.scratchpad_cells.iter().any(|c| c.id == cell_id) {
                        pf.errors.push(format!("Cell '{cell_id}' not found"));
                    }
                }
                Some(pf)
            }
            "scratchpad_wipe" => {
                let mut pf = PreFlightResult::new();
                if let Some(ids) = tool.input.get("cell_ids").and_then(|v| v.as_array())
                    && !ids.is_empty()
                {
                    let ss = ScratchpadState::get(state);
                    for id_val in ids {
                        if let Some(id) = id_val.as_str()
                            && !ss.scratchpad_cells.iter().any(|c| c.id == id)
                        {
                            pf.warnings.push(format!("Cell '{id}' not found — will be skipped"));
                        }
                    }
                }
                Some(pf)
            }
            _ => None,
        }
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "scratchpad_create_cell" => Some(tools::execute_create(tool, state)),
            "scratchpad_edit_cell" => Some(tools::execute_edit(tool, state)),
            "scratchpad_wipe" => Some(tools::execute_wipe(tool, state)),
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![
            ("scratchpad_create_cell", visualize_scratchpad_output),
            ("scratchpad_edit_cell", visualize_scratchpad_output),
            ("scratchpad_wipe", visualize_scratchpad_output),
        ]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::ContextTypeMeta> {
        vec![cp_base::state::ContextTypeMeta {
            context_type: "scratchpad",
            icon_id: "scratchpad",
            is_fixed: true,
            needs_cache: false,
            fixed_order: Some(8),
            display_name: "scratchpad",
            short_name: "scratch",
            needs_async_wait: false,
        }]
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Scratchpad", "A useful scratchpad for you to use however you like")]
    }
}

/// Visualizer for scratchpad tool results.
/// Highlights cell titles and shows creation vs edit vs deletion actions.
fn visualize_scratchpad_output(content: &str, width: usize) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::prelude::*;

    let success_color = Color::Rgb(80, 250, 123);
    let info_color = Color::Rgb(139, 233, 253);
    let error_color = Color::Rgb(255, 85, 85);
    let secondary_color = Color::Rgb(150, 150, 170);

    let mut lines = Vec::new();

    for line in content.lines() {
        if line.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        let style = if line.starts_with("Error:") {
            Style::default().fg(error_color)
        } else if line.starts_with("Created cell") {
            Style::default().fg(success_color)
        } else if line.starts_with("Updated") {
            Style::default().fg(info_color)
        } else if line.starts_with("Deleted") {
            Style::default().fg(error_color)
        } else if line.starts_with("C") && line.chars().nth(1).is_some_and(|c| c.is_ascii_digit()) {
            // Cell IDs like C1, C2
            Style::default().fg(info_color)
        } else if line.contains(":") {
            // Cell titles
            Style::default().fg(secondary_color)
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
