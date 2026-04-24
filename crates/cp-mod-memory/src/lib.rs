//! Memory module — persistent knowledge items across conversations.
//!
//! Two tools: `memory_create` and `memory_update` (modify/delete). Memories
//! survive across sessions and workers. Each has a tl;dr summary (capped at
//! 80 tokens) shown in the panel, with optional rich body text shown when opened.

/// Panel rendering and context generation for memory items.
mod panel;
/// Tool execution handlers for `memory_create` and `memory_update`.
mod tools;
/// Memory state types: `MemoryItem`, `MemoryImportance`, `MemoryState`.
pub mod types;

use types::MemoryState;

use cp_base::cast::Safe as _;

/// Maximum token length for memory `tl_dr` field (enforced on create/update)
pub const MEMORY_TLDR_MAX_TOKENS: usize = 80;

use serde_json::json;

use cp_base::modules::ToolVisualizer;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ParamType, ToolDefinition, ToolParam, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::MemoryPanel;
use cp_base::modules::Module;

/// Lazily parsed tool descriptions from the memory YAML definition.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/memory.yaml")));

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

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(Kind::MEMORY)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![(Kind::new(Kind::MEMORY), "Memories", false)]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            Kind::MEMORY => Some(Box::new(MemoryPanel)),
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

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<Verdict> {
        match tool.name.as_str() {
            "memory_create" => {
                let mut pf = Verdict::new();
                pf.activate_queue = true;
                Some(pf)
            }
            "memory_update" => {
                let mut pf = Verdict::new();
                pf.activate_queue = true;
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

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![cp_base::state::context::TypeMeta {
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

    fn dependencies(&self) -> &[&'static str] {
        &[]
    }

    fn is_core(&self) -> bool {
        false
    }

    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
        None
    }

    fn context_detail(&self, _ctx: &cp_base::state::context::Entry) -> Option<String> {
        None
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

/// Visualizer for memory tool results.
/// Colors importance levels and highlights created/updated memory summaries.
fn visualize_memory_output(content: &str, width: usize) -> Vec<cp_render::Block> {
    use cp_render::{Block, Span};

    content
        .lines()
        .map(|line| {
            if line.is_empty() {
                return Block::empty();
            }
            // Memory visualizer uses RGB for the importance gradient
            let rgb = if line.starts_with("Error:") {
                (255, 85, 85)
            } else if line.starts_with("Created") || line.starts_with("Updated") {
                (80, 250, 123)
            } else if line.contains("critical") {
                (255, 85, 85)
            } else if line.contains("high") {
                (255, 184, 108)
            } else if line.contains("medium") {
                (241, 250, 140)
            } else if line.contains("low")
                || (line.starts_with('M') && line.chars().nth(1).is_some_and(|c| c.is_ascii_digit()))
            {
                (139, 233, 253)
            } else {
                return Block::text(truncate_mem_line(line, width));
            };
            Block::Line(vec![Span::rgb(truncate_mem_line(line, width), rgb.0, rgb.1, rgb.2)])
        })
        .collect()
}

/// Truncate a line for memory visualizer output.
fn truncate_mem_line(line: &str, width: usize) -> String {
    if line.len() > width {
        format!("{}...", line.get(..line.floor_char_boundary(width.saturating_sub(3))).unwrap_or(""))
    } else {
        line.to_string()
    }
}
