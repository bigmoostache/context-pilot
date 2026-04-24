//! Firecrawl module — web scraping, search+scrape, and URL discovery.
//!
//! Three tools: `firecrawl_scrape` (single-URL extraction), `firecrawl_search`
//! (search and scrape in one call), `firecrawl_map` (site URL discovery).
//! Results appear as dynamic panels with full markdown content.

/// HTTP API client for Firecrawl scrape/search/map endpoints.
pub mod api;
/// Dynamic panel rendering for scraped content.
pub mod panel;
/// Tool dispatch: `firecrawl_scrape`, `firecrawl_search`, `firecrawl_map`.
pub mod tools;
/// Firecrawl API response/request serde types.
pub mod types;

use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::context::{Kind, TypeMeta};
use cp_base::state::runtime::State;
use cp_base::tools::{ParamType, ToolDefinition, ToolParam, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

/// Lazily-loaded tool description texts parsed from the YAML definition file.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/firecrawl.yaml")));

/// Firecrawl module: web scraping and content extraction via Firecrawl API.
#[derive(Debug, Clone, Copy)]
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

    fn context_type_metadata(&self) -> Vec<TypeMeta> {
        vec![TypeMeta {
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

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new("firecrawl_result")]
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

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        (context_type.as_str() == panel::FIRECRAWL_PANEL_TYPE).then(|| {
            let panel: Box<dyn Panel> = Box::new(panel::Results);
            panel
        })
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Web Scrape", "Web scraping and content extraction via Firecrawl")]
    }

    fn is_core(&self) -> bool {
        false
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

    fn pre_flight(&self, tool: &ToolUse, _state: &State) -> Option<cp_base::tools::pre_flight::Verdict> {
        match tool.name.as_str() {
            "firecrawl_scrape" | "firecrawl_search" | "firecrawl_map" => {
                let mut pf = cp_base::tools::pre_flight::Verdict::new();
                pf.activate_queue = true;
                Some(pf)
            }
            _ => None,
        }
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![]
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, cp_base::modules::ToolVisualizer)> {
        vec![]
    }

    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
        None
    }

    fn context_detail(&self, _ctx: &cp_base::state::context::Entry) -> Option<String> {
        None
    }

    fn overview_context_section(&self, _state: &State) -> Option<String> {
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
