//! Memory module — a fixed, tiered budget of memory slots across conversations.
//!
//! One tool: `memory_edit` (batch slot writes addressed by id `M-<tier>-<n>`).
//! Memories live in a **fixed budget of 220 slots** across five tiers
//! (`safe`/`tiny`/`short`/`mid`/`long`), each slot's `contents` bounded by its
//! tier. Editing a slot to empty frees it (renders `**empty**`); there is no
//! create/update/delete/move — only edit. Memories survive across sessions and
//! workers (shared `memories.yaml`).

/// Panel rendering and context generation for memory slots.
mod panel;
/// YAML-backed persistent storage + one-shot legacy migration.
mod storage;
/// Tool execution handler for `memory_edit`.
mod tools;
/// Memory state types: `Tier`, `MemorySlot`, `MemoryState`.
pub mod types;

use types::{MemoryState, TITLE_MAX_CHARS, TOTAL_SLOTS};

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

/// Memory module: a fixed tiered budget of memory slots.
#[derive(Debug, Clone, Copy)]
pub struct MemoryModule;

impl Default for MemoryModule {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryModule {
    /// Construct the module marker.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Module for MemoryModule {
    fn id(&self) -> &'static str {
        "memory"
    }
    fn name(&self) -> &'static str {
        "Memory"
    }
    fn description(&self) -> &'static str {
        "A fixed tiered budget of memory slots across conversations"
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
        json!({ "slots": ms.slots })
    }
    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        let ms = MemoryState::get_mut(state);
        if let Some(arr) = data.get("slots")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            ms.slots = v;
        }
        // YAML backing store: new slot-keyed entries, or one-shot legacy migration.
        storage::load_into(ms);
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
            ToolDefinition::from_yaml("memory_edit", t)
                .short_desc("Edit memory slots")
                .category("Memory")
                .reverie_allowed(true)
                .param_array(
                    "edits",
                    ParamType::Object(vec![
                        ToolParam::new("id", ParamType::String)
                            .desc("Slot id, M-<tier>-<n> (e.g. M-short-23)")
                            .required(),
                        ToolParam::new("title", ParamType::String).desc("Short label, <= 25 chars"),
                        ToolParam::new("contents", ParamType::String)
                            .desc("The value - dense and synthetic. Empty frees the slot."),
                        ToolParam::new("importance", ParamType::String)
                            .desc("Importance level")
                            .enum_vals(&["low", "medium", "high", "critical"]),
                    ]),
                    true,
                )
                .build(),
        ]
    }

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<Verdict> {
        if tool.name.as_str() != "memory_edit" {
            return None;
        }
        let mut pf = Verdict::new();
        if let Some(edits) = tool.input.get("edits").and_then(|v| v.as_array()) {
            let ms = MemoryState::get(state);
            for edit in edits {
                if let Some(id) = edit.get("id").and_then(|v| v.as_str())
                    && ms.slot(id).is_none()
                {
                    pf.errors.push(format!("Slot '{id}' does not exist"));
                }
            }
        }
        Some(pf)
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "memory_edit" => Some(tools::execute_edit(tool, state)),
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![("memory_edit", visualize_memory_output)]
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
        Some(format!("Memories: {}/{}\n", ms.occupied_count(), TOTAL_SLOTS))
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Memory", "Edit the fixed tiered budget of memory slots")]
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

    fn on_stream_chunk(&self, _text: &str, _state: &mut State) {}

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

/// Visualizer for the `memory_edit` tool result.
/// Colours importance levels and the Set/Freed/Errors summary lines.
fn visualize_memory_output(content: &str, width: usize) -> Vec<cp_render::Block> {
    use cp_render::{Block, Span};

    content
        .lines()
        .map(|line| {
            if line.is_empty() {
                return Block::empty();
            }
            let rgb = if line.starts_with("Errors") || line.starts_with("Missing") || line.starts_with("WARNING") {
                (255, 184, 108)
            } else if line.starts_with("Set") || line.starts_with("Freed") {
                (80, 250, 123)
            } else if line.contains("critical") {
                (255, 85, 85)
            } else if line.contains("high") {
                (255, 184, 108)
            } else if line.contains("medium") {
                (241, 250, 140)
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
        line.to_owned()
    }
}

/// Convenience re-export for downstream callers.
pub const MEMORY_TITLE_MAX_CHARS: usize = TITLE_MAX_CHARS;
