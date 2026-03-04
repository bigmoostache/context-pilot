//! Memory module — persistent knowledge items across conversations.
//!
//! Two tools: `memory_create` and `memory_update` (modify/delete). Memories
//! survive across sessions and workers. Each has a tl;dr summary (capped at
//! 80 tokens) shown in the panel, with optional rich body text shown when opened.

mod panel;
mod tools;
/// Memory state types: `MemoryItem`, `MemoryImportance`, `MemoryState`.
pub mod types;

use cp_base::cast::SafeCast;
pub use types::{MemoryImportance, MemoryItem, MemoryState};

/// Maximum token length for memory `tl_dr` field (enforced on create/update)
pub const MEMORY_TLDR_MAX_TOKENS: usize = 80;

use serde_json::json;

use cp_base::modules::ToolVisualizer;
use cp_base::panels::Panel;
use cp_base::state::{ContextType, State};
use cp_base::tools::{ParamType, PreFlightResult, ToolDefinition, ToolParam, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::MemoryPanel;
use cp_base::modules::Module;

static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> = std::sync::LazyLock::new(|| {
    serde_yaml::from_str(include_str!("../../../yamls/tools/memory.yaml")).expect("Failed to parse memory tool YAML")
});

/// Memory module: persistent knowledge items across conversations.
#[derive(Debug, Clone, Copy)]
pub struct MemoryModule;

impl Module for MemoryModule {
    fn id(&self) -> &'static str {
        "memory"
    }
    fn name(&self) -> &'static str {
        "Memory"
    }
    fn description(&self) -> &'static str {
        "Persistent memory items across conversations"
    }
    fn is_global(&self) -> bool {
        true
    }

    fn init_state(&self, state: &mut State) {
        state.set_ext(MemoryState::new());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(MemoryState::new());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let ms = MemoryState::get(state);
        json!({
            "memories": ms.memories,
            "next_memory_id": ms.next_memory_id,
            "open_memory_ids": ms.open_memory_ids,
        })
    }
    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        let ms = MemoryState::get_mut(state);
        if let Some(arr) = data.get("memories")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            ms.memories = v;
        }
        if let Some(v) = data.get("next_memory_id").and_then(serde_json::Value::as_u64) {
            ms.next_memory_id = v.to_usize();
        }
        if let Some(arr) = data.get("open_memory_ids")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            ms.open_memory_ids = v;
        }
    }

    fn fixed_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::new(ContextType::MEMORY)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> {
        vec![(ContextType::new(ContextType::MEMORY), "Memories", false)]
    }

    fn create_panel(&self, context_type: &ContextType) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            ContextType::MEMORY => Some(Box::new(MemoryPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("memory_create", t)
                .short_desc("Store persistent memories")
                .category("Memory")
                .reverie_allowed(true)
                .param_array(
                    "memories",
                    ParamType::Object(vec![
                        ToolParam::new("content", ParamType::String).desc("Memory content").required(),
                        ToolParam::new("contents", ParamType::String)
                            .desc("Rich body text (visible when memory is opened)"),
                        ToolParam::new("importance", ParamType::String)
                            .desc("Importance level")
                            .enum_vals(&["low", "medium", "high", "critical"]),
                        ToolParam::new("labels", ParamType::Array(Box::new(ParamType::String)))
                            .desc("Freeform labels for categorization (e.g., ['architecture', 'bug'])"),
                    ]),
                    true,
                )
                .build(),
            ToolDefinition::from_yaml("memory_update", t)
                .short_desc("Modify stored notes")
                .category("Memory")
                .reverie_allowed(true)
                .param_array(
                    "updates",
                    ParamType::Object(vec![
                        ToolParam::new("id", ParamType::String).desc("Memory ID (e.g., M1)").required(),
                        ToolParam::new("content", ParamType::String).desc("New content"),
                        ToolParam::new("contents", ParamType::String)
                            .desc("New rich body text (visible when memory is opened)"),
                        ToolParam::new("importance", ParamType::String)
                            .desc("New importance level")
                            .enum_vals(&["low", "medium", "high", "critical"]),
                        ToolParam::new("labels", ParamType::Array(Box::new(ParamType::String)))
                            .desc("New labels (replaces existing)"),
                        ToolParam::new("open", ParamType::Boolean)
                            .desc("Set true to show full contents in panel, false to show only tl;dr"),
                        ToolParam::new("delete", ParamType::Boolean).desc("Set true to delete"),
                    ]),
                    true,
                )
                .build(),
        ]
    }

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<PreFlightResult> {
        match tool.name.as_str() {
            "memory_update" => {
                let mut pf = PreFlightResult::new();
                if let Some(updates) = tool.input.get("updates").and_then(|v| v.as_array()) {
                    let ms = MemoryState::get(state);
                    for update in updates {
                        if let Some(id) = update.get("id").and_then(|v| v.as_str())
                            && !ms.memories.iter().any(|m| m.id == id)
                        {
                            pf.errors.push(format!("Memory '{id}' not found"));
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
            "memory_create" => Some(tools::execute_create(tool, state)),
            "memory_update" => Some(tools::execute_update(tool, state)),
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![("memory_create", visualize_memory_output), ("memory_update", visualize_memory_output)]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::ContextTypeMeta> {
        vec![cp_base::state::ContextTypeMeta {
            context_type: "memory",
            icon_id: "memory",
            is_fixed: true,
            needs_cache: false,
            fixed_order: Some(4),
            display_name: "memory",
            short_name: "memories",
            needs_async_wait: false,
        }]
    }

    fn overview_context_section(&self, state: &State) -> Option<String> {
        let ms = MemoryState::get(state);
        if ms.memories.is_empty() {
            return None;
        }
        Some(format!("Memories: {}\n", ms.memories.len()))
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Memory", "Store persistent memories across the conversation")]
    }
}

/// Visualizer for memory tool results.
/// Colors importance levels and highlights created/updated memory summaries.
fn visualize_memory_output(content: &str, width: usize) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::prelude::*;

    let critical_color = Color::Rgb(255, 85, 85); // Red for critical
    let high_color = Color::Rgb(255, 184, 108); // Orange for high
    let medium_color = Color::Rgb(241, 250, 140); // Yellow for medium
    let low_color = Color::Rgb(139, 233, 253); // Cyan for low
    let success_color = Color::Rgb(80, 250, 123); // Green for success messages
    let error_color = Color::Rgb(255, 85, 85); // Red for errors

    let mut lines = Vec::new();

    for line in content.lines() {
        if line.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        let style = if line.starts_with("Error:") {
            Style::default().fg(error_color)
        } else if line.starts_with("Created") || line.starts_with("Updated") {
            Style::default().fg(success_color)
        } else if line.contains("critical") {
            Style::default().fg(critical_color)
        } else if line.contains("high") {
            Style::default().fg(high_color)
        } else if line.contains("medium") {
            Style::default().fg(medium_color)
        } else if line.contains("low") {
            Style::default().fg(low_color)
        } else if line.starts_with("M") && line.chars().nth(1).is_some_and(|c| c.is_ascii_digit()) {
            // Memory IDs like M1, M2
            Style::default().fg(low_color)
        } else {
            Style::default()
        };

        let display = if line.len() > width {
            format!("{}...", &line[..line.floor_char_boundary(width.saturating_sub(3))])
        } else {
            line.to_string()
        };
        lines.push(Line::from(Span::styled(display, style)));
    }

    lines
}
