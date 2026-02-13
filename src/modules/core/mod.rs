pub mod conversation;
mod conversation_history_panel;
mod conversation_list;
mod conversation_panel;
pub(crate) mod conversation_render;
mod overview_context;
mod overview_panel;
mod overview_render;
mod tools;

use serde_json::json;

use crate::core::panels::Panel;
use crate::state::{ContextType, State};
use crate::tool_defs::{ToolDefinition, ToolParam, ParamType, ToolCategory};
use crate::tools::{ToolUse, ToolResult};

use self::conversation_panel::ConversationPanel;
use self::overview_panel::OverviewPanel;
use super::Module;

pub struct CoreModule;

impl Module for CoreModule {
    fn id(&self) -> &'static str { "core" }
    fn name(&self) -> &'static str { "Core" }
    fn description(&self) -> &'static str { "Essential context and system tools" }
    fn is_core(&self) -> bool { true }
    fn is_global(&self) -> bool { true }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        json!({
            "active_modules": state.active_modules.iter().collect::<Vec<_>>(),
            "dev_mode": state.dev_mode,
            "llm_provider": state.llm_provider,
            "anthropic_model": state.anthropic_model,
            "grok_model": state.grok_model,
            "groq_model": state.groq_model,
            "deepseek_model": state.deepseek_model,
            "cleaning_threshold": state.cleaning_threshold,
            "cleaning_target_proportion": state.cleaning_target_proportion,
            "context_budget": state.context_budget,
            "global_next_uid": state.global_next_uid,
            "cache_hit_tokens": state.cache_hit_tokens,
            "cache_miss_tokens": state.cache_miss_tokens,
            "total_output_tokens": state.total_output_tokens,
            "disabled_tools": state.tools.iter().filter(|t| !t.enabled).map(|t| &t.id).collect::<Vec<_>>(),
        })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(arr) = data.get("active_modules").and_then(|v| v.as_array()) {
            state.active_modules = arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            // Auto-add newly introduced modules that aren't in the persisted config.
            // This handles the case where a new module is added to the codebase but
            // the user's config.json was written before it existed.
            let all_defaults = crate::modules::default_active_modules();
            for module_id in &all_defaults {
                if !state.active_modules.contains(module_id) {
                    state.active_modules.insert(module_id.clone());
                }
            }
        }
        if let Some(v) = data.get("dev_mode").and_then(|v| v.as_bool()) {
            state.dev_mode = v;
        }
        if let Some(v) = data.get("llm_provider") {
            if let Ok(p) = serde_json::from_value(v.clone()) {
                state.llm_provider = p;
            }
        }
        if let Some(v) = data.get("anthropic_model") {
            if let Ok(m) = serde_json::from_value(v.clone()) {
                state.anthropic_model = m;
            }
        }
        if let Some(v) = data.get("grok_model") {
            if let Ok(m) = serde_json::from_value(v.clone()) {
                state.grok_model = m;
            }
        }
        if let Some(v) = data.get("groq_model") {
            if let Ok(m) = serde_json::from_value(v.clone()) {
                state.groq_model = m;
            }
        }
        if let Some(v) = data.get("deepseek_model") {
            if let Ok(m) = serde_json::from_value(v.clone()) {
                state.deepseek_model = m;
            }
        }
        if let Some(v) = data.get("cleaning_threshold").and_then(|v| v.as_f64()) {
            state.cleaning_threshold = v as f32;
        }
        if let Some(v) = data.get("cleaning_target_proportion").and_then(|v| v.as_f64()) {
            state.cleaning_target_proportion = v as f32;
        }
        if let Some(v) = data.get("context_budget") {
            state.context_budget = v.as_u64().map(|n| n as usize);
        }
        if let Some(v) = data.get("global_next_uid").and_then(|v| v.as_u64()) {
            state.global_next_uid = v as usize;
        }
        if let Some(v) = data.get("cache_hit_tokens").and_then(|v| v.as_u64()) {
            state.cache_hit_tokens = v as usize;
        }
        if let Some(v) = data.get("cache_miss_tokens").and_then(|v| v.as_u64()) {
            state.cache_miss_tokens = v as usize;
        }
        if let Some(v) = data.get("total_output_tokens").and_then(|v| v.as_u64()) {
            state.total_output_tokens = v as usize;
        }
        if let Some(arr) = data.get("disabled_tools").and_then(|v| v.as_array()) {
            let disabled: Vec<String> = arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            // Build tools from active_modules (must be loaded already) and apply disabled state
            state.tools = crate::modules::active_tool_definitions(&state.active_modules);
            for tool in &mut state.tools {
                if tool.id != "tool_manage" && tool.id != "module_toggle" && disabled.contains(&tool.id) {
                    tool.enabled = false;
                }
            }
        }
    }

    fn fixed_panel_types(&self) -> Vec<ContextType> {
        vec![
            ContextType::Overview,
        ]
    }

    fn dynamic_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::ConversationHistory]
    }

    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> {
        vec![
            (ContextType::Overview, "World", false),
        ]
    }

    fn create_panel(&self, context_type: ContextType) -> Option<Box<dyn Panel>> {
        match context_type {
            ContextType::Conversation => Some(Box::new(ConversationPanel)),
            ContextType::Overview => Some(Box::new(OverviewPanel)),
            ContextType::ConversationHistory => Some(Box::new(
                conversation_history_panel::ConversationHistoryPanel,
            )),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let mut defs = vec![
            // Context tools
            ToolDefinition {
                id: "context_close".to_string(),
                name: "Close Contexts".to_string(),
                short_desc: "Remove items from context".to_string(),
                description: "Closes context elements by their IDs (e.g., P6, P7). Cannot close core elements (P1-P6).".to_string(),
                params: vec![
                    ToolParam::new("ids", ParamType::Array(Box::new(ParamType::String)))
                        .desc("List of context IDs to close")
                        .required(),
                ],
                enabled: true,
                category: ToolCategory::Context,
            },

            // System tools (reload stays in core)
            ToolDefinition {
                id: "system_reload".to_string(),
                name: "Reload TUI".to_string(),
                short_desc: "Restart the TUI".to_string(),
                description: "Reloads the TUI application to apply changes. Use after modifying TUI source code and rebuilding. State is preserved. IMPORTANT: You must ALWAYS call this tool after building - never just say 'reloading' without actually invoking this tool.".to_string(),
                params: vec![],
                enabled: true,
                category: ToolCategory::System,
            },

            // Meta tools
            ToolDefinition {
                id: "tool_manage".to_string(),
                name: "Manage Tools".to_string(),
                short_desc: "Enable/disable tools".to_string(),
                description: "Enables or disables tools. This tool cannot be disabled. Use to customize available capabilities.".to_string(),
                params: vec![
                    ToolParam::new("changes", ParamType::Array(Box::new(ParamType::Object(vec![
                        ToolParam::new("tool", ParamType::String)
                            .desc("Tool ID to change (e.g., 'edit_file', 'glob')")
                            .required(),
                        ToolParam::new("action", ParamType::String)
                            .desc("Action to perform")
                            .enum_vals(&["enable", "disable"])
                            .required(),
                    ]))))
                        .desc("Array of tool changes")
                        .required(),
                ],
                enabled: true,
                category: ToolCategory::System,
            },
        ];

        // Panel pagination tool (dynamically enabled/disabled)
        defs.push(ToolDefinition {
            id: "panel_goto_page".to_string(),
            name: "Go To Page".to_string(),
            short_desc: "Navigate paginated panel".to_string(),
            description: "Navigates to a specific page of a paginated panel. Only available when a panel has more than one page.".to_string(),
            params: vec![
                ToolParam::new("panel_id", ParamType::String)
                    .desc("Panel ID (e.g., P8)")
                    .required(),
                ToolParam::new("page", ParamType::Integer)
                    .desc("Page number (1-indexed)")
                    .required(),
            ],
            enabled: false,
            category: ToolCategory::Context,
        });

        // Add module_toggle tool
        defs.push(super::module_toggle_tool_definition());

        defs
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            // Context tools
            "context_close" => Some(self::tools::close_context::execute(tool, state)),
            "panel_goto_page" => Some(self::tools::panel_goto_page::execute(tool, state)),

            // System tools (reload stays in core)
            "system_reload" => Some(crate::tools::execute_reload_tui(tool, state)),

            // Meta tools
            "tool_manage" => Some(self::tools::manage_tools::execute(tool, state)),

            // module_toggle is handled in dispatch_tool() directly
            _ => None,
        }
    }
}
