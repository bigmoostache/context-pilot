/// Tool implementation for the `Think` reasoning tool.
mod think;
pub(crate) use think::ThinkState;

use crate::app::panels::Panel;
use crate::infra::tools::{ParamType, ToolDefinition, ToolTexts};
use crate::infra::tools::{ToolResult, ToolUse};
use crate::state::{Kind, State};

use super::Module;

/// Lazily parsed tool text definitions for core tools (used by `Think`).
static CORE_TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/core.yaml")));

/// Module that provides the Think reasoning tool.
pub(crate) struct QuestionsModule;

impl Module for QuestionsModule {
    fn id(&self) -> &'static str {
        "questions"
    }
    fn name(&self) -> &'static str {
        "Questions"
    }
    fn description(&self) -> &'static str {
        "Interactive user question forms"
    }
    fn is_core(&self) -> bool {
        true
    }
    fn is_global(&self) -> bool {
        true
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Context", "Manage conversation context and system prompts")]
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let core_t = &*CORE_TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("Think", core_t)
                .short_desc("Record a structured reasoning step")
                .category("Context")
                .param("thought_body", ParamType::String, true)
                .param("task_context", ParamType::String, false)
                .build(),
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "Think" => Some(think::execute(tool, state)),
            _ => None,
        }
    }

    fn create_panel(&self, _context_type: &Kind) -> Option<Box<dyn Panel>> {
        None
    }

    fn dependencies(&self) -> &[&'static str] {
        &[]
    }

    fn init_state(&self, state: &mut State) {
        state.set_ext(ThinkState::default());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(ThinkState::default());
    }

    fn save_module_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_module_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn save_worker_data(&self, state: &State) -> serde_json::Value {
        state
            .get_ext::<ThinkState>()
            .map_or(serde_json::Value::Null, |ts| serde_json::to_value(ts).unwrap_or(serde_json::Value::Null))
    }

    fn load_worker_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Ok(ts) = serde_json::from_value::<ThinkState>(data.clone()) {
            state.set_ext(ts);
        }
    }

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

    fn context_type_metadata(&self) -> Vec<crate::state::TypeMeta> {
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

    fn on_user_message(&self, _state: &mut State) {}

    fn on_stream_stop(&self, _state: &mut State) {}

    fn on_stream_chunk(&self, _text: &str, _state: &mut State) {}

    fn on_tool_progress(&self, _tool_name: &str, _input_so_far: &str, _state: &mut State) {}

    fn on_tool_complete(&self, tool_name: &str, state: &mut State) {
        if tool_name == "Think" {
            return;
        }
        // Non-Think tool: drift counter toward (and below) zero
        let fire = {
            let ts = state.ext_mut::<ThinkState>();
            ts.consecutive_count = ts.consecutive_count.saturating_sub(1).min(0i32);
            // Check if we've hit the next notification point
            if ts.consecutive_count == ts.next_notification_at {
                ts.next_notification_at = ts.next_notification_at.saturating_add(ts.reminder_threshold);
                true
            } else {
                false
            }
        };
        if fire {
            let id = cp_mod_spine::types::SpineState::create_notification(
                state,
                cp_mod_spine::types::NotificationType::Custom,
                "Think Reminder".into(),
                "Please think more. Thinking is both cheap in tokens, and drastically \
                 augments your performances. Make a habit out of it."
                    .into(),
            );
            // Auto-mark as processed — the nudge is injected into the chat
            // stream but should not accumulate in the Spine panel or trigger
            // auto-continuation.
            let _found = cp_mod_spine::types::SpineState::mark_notification_processed(state, &id);
        }
    }

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
