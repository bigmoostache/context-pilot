pub mod api;
pub mod panel;
pub mod tools;
pub mod types;

use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::{ContextType, ContextTypeMeta, State};
use cp_base::tools::{ParamType, ToolDefinition, ToolParam, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> = std::sync::LazyLock::new(|| {
    serde_yaml::from_str(include_str!("../../../yamls/tools/firecrawl.yaml"))
        .expect("Failed to parse firecrawl tool YAML")
});

#[derive(Debug)]
pub struct FirecrawlModule;

impl Module for FirecrawlModule {
    fn id(&self) -> &'static str {
        "firecrawl"
    }

    fn name(&self) -> &'static str {
        "Firecrawl"
    }

    fn description(&self) -> &'static str {
        "Web scraping and content extraction via Firecrawl API"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["core"]
    }

    fn is_global(&self) -> bool {
        true
    }

    fn context_type_metadata(&self) -> Vec<ContextTypeMeta> {
        vec![ContextTypeMeta {
            context_type: "firecrawl_result",
            icon_id: "scrape",
            is_fixed: false,
            needs_cache: false,
            fixed_order: None,
            display_name: "firecrawl",
            short_name: "firecrawl",
            needs_async_wait: false,
        }]
    }

    fn dynamic_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::new("firecrawl_result")]
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("firecrawl_scrape", t)
                .short_desc("Scrape a URL for full content")
                .category("Web Scrape")
                .param("url", ParamType::String, true)
                .param_array("formats", ParamType::String, false)
                .param_object(
                    "location",
                    vec![
                        ToolParam::new("country", ParamType::String),
                        ToolParam::new("languages", ParamType::Array(Box::new(ParamType::String))),
                    ],
                    false,
                )
                .build(),
            ToolDefinition::from_yaml("firecrawl_search", t)
                .short_desc("Search and scrape in one call")
                .category("Web Scrape")
                .param("query", ParamType::String, true)
                .param("limit", ParamType::Integer, false)
                .param_array("sources", ParamType::String, false)
                .param_array("categories", ParamType::String, false)
                .param("tbs", ParamType::String, false)
                .param("location", ParamType::String, false)
                .build(),
            ToolDefinition::from_yaml("firecrawl_map", t)
                .short_desc("Discover all URLs on a domain")
                .category("Web Scrape")
                .param("url", ParamType::String, true)
                .param("limit", ParamType::Integer, false)
                .param("search", ParamType::String, false)
                .param("include_subdomains", ParamType::Boolean, false)
                .param_object(
                    "location",
                    vec![
                        ToolParam::new("country", ParamType::String),
                        ToolParam::new("languages", ParamType::Array(Box::new(ParamType::String))),
                    ],
                    false,
                )
                .build(),
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        tools::dispatch(tool, state)
    }

    fn create_panel(&self, context_type: &ContextType) -> Option<Box<dyn Panel>> {
        if context_type.as_str() == panel::FIRECRAWL_PANEL_TYPE {
            Some(Box::new(panel::FirecrawlResultPanel))
        } else {
            None
        }
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Web Scrape", "Web scraping and content extraction via Firecrawl")]
    }
}
