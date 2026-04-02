/// List continuation detection for input editing.
mod list;
/// IR-native markdown parser for conversation messages.
mod markdown_ir;
/// Panel implementation for conversation display.
mod panel;
/// Token estimation and context refresh for conversation.
pub(crate) mod refresh;
/// IR-based message renderer emitting `Vec<Block>` instead of ratatui lines.
pub(crate) mod render_blocks;
/// IR-based input area renderer emitting `Vec<Block>`.
mod render_input_blocks;
/// Best-effort JSON field extraction for streaming tool call display.
mod render_json;

pub(crate) use panel::build_content_cached;

use crate::app::panels::Panel;
use crate::infra::tools::{ToolDefinition, ToolResult, ToolUse};
use crate::state::{Kind, State, TypeMeta};

use self::panel::ConversationPanel;
use super::Module;

/// Module that handles the main conversation panel display and input.
pub(crate) struct ConversationModule;

impl Module for ConversationModule {
    fn id(&self) -> &'static str {
        "conversation_panel"
    }
    fn name(&self) -> &'static str {
        "Conversation"
    }
    fn description(&self) -> &'static str {
        "Conversation display and input"
    }
    fn is_core(&self) -> bool {
        true
    }
    fn is_global(&self) -> bool {
        true
    }

    fn context_type_metadata(&self) -> Vec<TypeMeta> {
        vec![
            TypeMeta {
                context_type: "conversation",
                icon_id: "conversation",
                is_fixed: false,
                needs_cache: false,
                fixed_order: None,
                display_name: "conversation",
                short_name: "chat",
                needs_async_wait: false,
            },
            TypeMeta {
                context_type: "system",
                icon_id: "system",
                is_fixed: false,
                needs_cache: false,
                fixed_order: None,
                display_name: "system",
                short_name: "seed",
                needs_async_wait: false,
            },
        ]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            Kind::CONVERSATION => Some(Box::new(ConversationPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![]
    }

    fn execute_tool(&self, _tool: &ToolUse, _state: &mut State) -> Option<ToolResult> {
        None
    }

    fn dependencies(&self) -> &[&'static str] {
        &[]
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

    fn pre_flight(&self, _tool: &ToolUse, _state: &State) -> Option<crate::infra::tools::Verdict> {
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

    fn tool_visualizers(&self) -> Vec<(&'static str, super::ToolVisualizer)> {
        vec![]
    }

    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
        None
    }

    fn context_detail(&self, _ctx: &crate::state::Entry) -> Option<String> {
        None
    }

    fn overview_context_section(&self, _state: &State) -> Option<String> {
        None
    }

    fn overview_render_sections(&self, _state: &State) -> Vec<(u8, Vec<cp_render::Block>)> {
        vec![]
    }

    fn on_close_context(&self, _ctx: &crate::state::Entry, _state: &mut State) -> Option<Result<String, String>> {
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
        _ctx: &crate::state::Entry,
        _changed_path: &str,
        _is_dir_event: bool,
    ) -> bool {
        false
    }

    fn watcher_immediate_refresh(&self) -> bool {
        true
    }
}
