mod list;
mod panel;
pub(crate) mod refresh;
pub(crate) mod render;
mod render_input;

use crate::app::panels::Panel;
use crate::infra::tools::{ToolDefinition, ToolResult, ToolUse};
use crate::state::{ContextType, ContextTypeMeta, State};

use self::panel::ConversationPanel;
use super::Module;

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

    fn context_type_metadata(&self) -> Vec<ContextTypeMeta> {
        vec![
            ContextTypeMeta {
                context_type: "conversation",
                icon_id: "conversation",
                is_fixed: false,
                needs_cache: false,
                fixed_order: None,
                display_name: "conversation",
                short_name: "chat",
                needs_async_wait: false,
            },
            ContextTypeMeta {
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

    fn create_panel(&self, context_type: &ContextType) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            ContextType::CONVERSATION => Some(Box::new(ConversationPanel)),
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
