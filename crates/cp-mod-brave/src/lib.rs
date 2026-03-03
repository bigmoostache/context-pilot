pub mod api;
pub mod panel;
pub mod tools;
pub mod types;

use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::{ContextType, ContextTypeMeta, State};
use cp_base::tools::{ParamType, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> = std::sync::LazyLock::new(|| {
    serde_yaml::from_str(include_str!("../../../yamls/tools/brave.yaml")).expect("Failed to parse brave tool YAML")
});

pub struct BraveModule;

impl Module for BraveModule {
    fn id(&self) -> &'static str {
        "brave"
    }

    fn name(&self) -> &'static str {
        "Brave Search"
    }

    fn description(&self) -> &'static str {
        "Web search and LLM context via Brave Search API"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["core"]
    }

    fn is_global(&self) -> bool {
        true
    }

    fn context_type_metadata(&self) -> Vec<ContextTypeMeta> {
        vec![ContextTypeMeta {
            context_type: "brave_result",
            icon_id: "search",
            is_fixed: false,
            needs_cache: false,
            fixed_order: None,
            display_name: "brave",
            short_name: "brave",
            needs_async_wait: false,
        }]
    }

    fn dynamic_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::new("brave_result")]
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("brave_search", t)
                .short_desc("Search the web via Brave")
                .category("Web Search")
                .param("query", ParamType::String, true)
                .param("count", ParamType::Integer, false)
                .param("freshness", ParamType::String, false)
                .param("country", ParamType::String, false)
                .param("search_lang", ParamType::String, false)
                .param_enum("safe_search", &["off", "moderate", "strict"], false)
                .param("goggles_id", ParamType::String, false)
                .build(),
            ToolDefinition::from_yaml("brave_llm_context", t)
                .short_desc("Get LLM-optimized web content from Brave")
                .category("Web Search")
                .param("query", ParamType::String, true)
                .param("maximum_number_of_tokens", ParamType::Integer, false)
                .param("count", ParamType::Integer, false)
                .param_enum("context_threshold_mode", &["strict", "balanced", "lenient", "disabled"], false)
                .param("freshness", ParamType::String, false)
                .param("country", ParamType::String, false)
                .param("goggles", ParamType::String, false)
                .build(),
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        tools::dispatch(tool, state)
    }

    fn create_panel(&self, context_type: &ContextType) -> Option<Box<dyn Panel>> {
        if context_type.as_str() == panel::BRAVE_PANEL_TYPE { Some(Box::new(panel::BraveResultPanel)) } else { None }
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Web Search", "Search the web via Brave Search API")]
    }
}
