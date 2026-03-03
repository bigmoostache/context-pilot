mod panel;

use crate::app::panels::Panel;
use crate::infra::tools::{ToolDefinition, ToolResult, ToolUse};
use crate::state::{ContextType, ContextTypeMeta, State};
use cp_base::config::INJECTIONS;

use self::panel::ConversationHistoryPanel;
use super::Module;

pub struct ConversationHistoryModule;

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

    fn context_type_metadata(&self) -> Vec<ContextTypeMeta> {
        vec![ContextTypeMeta {
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

    fn dynamic_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::new(ContextType::CONVERSATION_HISTORY)]
    }

    fn on_close_context(
        &self,
        ctx: &crate::state::ContextElement,
        _state: &mut State,
    ) -> Option<Result<String, String>> {
        if ctx.context_type.as_str() == ContextType::CONVERSATION_HISTORY {
            let msg = INJECTIONS.redirects.conversation_history_close.trim_end().replace("{id}", &ctx.id);
            return Some(Err(msg));
        }
        None
    }

    fn create_panel(&self, context_type: &ContextType) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            ContextType::CONVERSATION_HISTORY => Some(Box::new(ConversationHistoryPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![]
    }

    fn execute_tool(&self, _tool: &ToolUse, _state: &mut State) -> Option<ToolResult> {
        None
    }
}
