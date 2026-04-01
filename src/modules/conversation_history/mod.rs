/// Panel implementation for frozen conversation history display.
mod panel;

use crate::app::panels::Panel;
use crate::infra::tools::{ToolDefinition, ToolResult, ToolUse};
use crate::state::{Kind, State, TypeMeta};
use cp_base::config::INJECTIONS;

use self::panel::ConversationHistoryPanel;
use super::Module;

/// Module that manages frozen conversation history chunks.
pub(crate) struct ConversationHistoryModule;

impl Module for ConversationHistoryModule {
    fn id(&self) -> &'static str {
        "conversation_history_panel"
    }
    fn name(&self) -> &'static str {
        "Conversation History"
    }
    fn description(&self) -> &'static str {
        "Frozen conversation history chunks"
    }
    fn is_core(&self) -> bool {
        true
    }
    fn is_global(&self) -> bool {
        true
    }

    fn context_type_metadata(&self) -> Vec<TypeMeta> {
        vec![TypeMeta {
            context_type: "conversation_history",
            icon_id: "conversation",
            is_fixed: false,
            needs_cache: false,
            fixed_order: None,
            display_name: "chat-history",
            short_name: "history",
            needs_async_wait: false,
        }]
    }

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(Kind::CONVERSATION_HISTORY)]
    }

    fn on_close_context(&self, ctx: &crate::state::Entry, _state: &mut State) -> Option<Result<String, String>> {
        if ctx.context_type.as_str() == Kind::CONVERSATION_HISTORY {
            let msg = INJECTIONS.redirects.conversation_history_close.trim_end().replace("{id}", &ctx.id);
            return Some(Err(msg));
        }
        None
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            Kind::CONVERSATION_HISTORY => Some(Box::new(ConversationHistoryPanel)),
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
