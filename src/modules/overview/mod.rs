/// IR block builders for the overview panel sections.
mod blocks;
/// Context content generation for the LLM overview.
pub(crate) mod context;
/// Panel implementation for the overview statistics view.
mod panel;
/// Tool implementations for context management.
mod tools;
/// IR block builders for the tools/configuration panel sections.
mod tools_blocks;
/// Panel for tools/configuration display.
mod tools_panel;
/// Tool result visualizers for core tools.
mod visualizers;

use serde_json::json;

use crate::app::panels::Panel;
use crate::infra::tools::{ParamType, ToolDefinition, ToolParam, ToolTexts, Verdict};
use crate::infra::tools::{ToolResult, ToolUse};
use crate::modules::ToolVisualizer;
use crate::state::{Kind, State, TypeMeta};

use self::panel::OverviewPanel;
use self::tools_panel::ToolsPanel;
use super::Module;
use cp_base::cast::Safe;

/// Lazily parsed tool text definitions for core tools.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/core.yaml")));

/// Module that provides the overview panel, tools panel, and system tools.
pub(crate) struct OverviewModule;

impl Module for OverviewModule {
    fn id(&self) -> &'static str {
        "core"
    }
    fn name(&self) -> &'static str {
        "Overview"
    }
    fn description(&self) -> &'static str {
        "Overview panel and system tools"
    }
    fn is_core(&self) -> bool {
        true
    }
    fn is_global(&self) -> bool {
        true
    }

    fn save_worker_data(&self, state: &State) -> serde_json::Value {
        json!({
            "previous_panel_hash_list": state.previous_panel_hash_list,
        })
    }

    fn load_worker_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(arr) = data.get("previous_panel_hash_list").and_then(|v| v.as_array()) {
            state.previous_panel_hash_list = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
        }
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        json!({
            "active_modules": state.active_modules.iter().collect::<Vec<_>>(),
            "dev_mode": state.flags.ui.dev_mode,
            "llm_provider": state.llm_provider,
            "anthropic_model": state.anthropic_model,
            "grok_model": state.grok_model,
            "groq_model": state.groq_model,
            "deepseek_model": state.deepseek_model,
            "minimax_model": state.minimax_model,
            "claude_code_v2_model": state.claude_code_v2_model,
            "secondary_provider": state.secondary_provider,
            "secondary_anthropic_model": state.secondary_anthropic_model,
            "secondary_grok_model": state.secondary_grok_model,
            "secondary_groq_model": state.secondary_groq_model,
            "secondary_deepseek_model": state.secondary_deepseek_model,
            "secondary_minimax_model": state.secondary_minimax_model,
            "secondary_claude_code_v2_model": state.secondary_claude_code_v2_model,
            "reverie_enabled": state.flags.config.reverie_enabled,
            "cleaning_threshold": state.cleaning_threshold,
            "context_budget": state.context_budget,
            "global_next_uid": state.global_next_uid,
            "cache_hit_tokens": state.cache_hit_tokens,
            "cache_miss_tokens": state.cache_miss_tokens,
            "total_output_tokens": state.total_output_tokens,
            "cost_hit_usd": state.cost_hit_usd,
            "cost_miss_usd": state.cost_miss_usd,
            "cost_output_usd": state.cost_output_usd,
            "disabled_tools": state.tools.iter().filter(|t| !t.enabled).map(|t| &t.id).collect::<Vec<_>>(),
        })
    }
    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(arr) = data.get("active_modules").and_then(|v| v.as_array()) {
            state.active_modules = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
            // Auto-add newly introduced modules that aren't in the persisted config.
            let mut all_defaults: Vec<_> = crate::modules::default_active_modules().into_iter().collect();
            all_defaults.sort();
            for module_id in &all_defaults {
                if !state.active_modules.contains(module_id) {
                    let _r = state.active_modules.insert(module_id.clone());
                }
            }
        }
        if let Some(v) = data.get("dev_mode").and_then(serde_json::Value::as_bool) {
            state.flags.ui.dev_mode = v;
        }
        if let Some(v) = data.get("llm_provider")
            && let Ok(p) = serde_json::from_value(v.clone())
        {
            state.llm_provider = p;
        }
        if let Some(v) = data.get("anthropic_model")
            && let Ok(m) = serde_json::from_value(v.clone())
        {
            state.anthropic_model = m;
        }
        if let Some(v) = data.get("grok_model")
            && let Ok(m) = serde_json::from_value(v.clone())
        {
            state.grok_model = m;
        }
        if let Some(v) = data.get("groq_model")
            && let Ok(m) = serde_json::from_value(v.clone())
        {
            state.groq_model = m;
        }
        if let Some(v) = data.get("deepseek_model")
            && let Ok(m) = serde_json::from_value(v.clone())
        {
            state.deepseek_model = m;
        }
        if let Some(v) = data.get("minimax_model")
            && let Ok(m) = serde_json::from_value(v.clone())
        {
            state.minimax_model = m;
        }
        if let Some(v) = data.get("claude_code_v2_model")
            && let Ok(m) = serde_json::from_value(v.clone())
        {
            state.claude_code_v2_model = m;
        }
        if let Some(v) = data.get("secondary_provider")
            && let Ok(p) = serde_json::from_value(v.clone())
        {
            state.secondary_provider = p;
        }
        if let Some(v) = data.get("secondary_anthropic_model")
            && let Ok(m) = serde_json::from_value(v.clone())
        {
            state.secondary_anthropic_model = m;
        }
        if let Some(v) = data.get("secondary_grok_model")
            && let Ok(m) = serde_json::from_value(v.clone())
        {
            state.secondary_grok_model = m;
        }
        if let Some(v) = data.get("secondary_groq_model")
            && let Ok(m) = serde_json::from_value(v.clone())
        {
            state.secondary_groq_model = m;
        }
        if let Some(v) = data.get("secondary_deepseek_model")
            && let Ok(m) = serde_json::from_value(v.clone())
        {
            state.secondary_deepseek_model = m;
        }
        if let Some(v) = data.get("secondary_minimax_model")
            && let Ok(m) = serde_json::from_value(v.clone())
        {
            state.secondary_minimax_model = m;
        }
        if let Some(v) = data.get("secondary_claude_code_v2_model")
            && let Ok(m) = serde_json::from_value(v.clone())
        {
            state.secondary_claude_code_v2_model = m;
        }
        if let Some(v) = data.get("reverie_enabled").and_then(serde_json::Value::as_bool) {
            state.flags.config.reverie_enabled = v;
        }
        if let Some(v) = data.get("cleaning_threshold").and_then(serde_json::Value::as_f64) {
            state.cleaning_threshold = v.to_f32();
        }
        if let Some(v) = data.get("context_budget") {
            state.context_budget = v.as_u64().map(Safe::to_usize);
        }
        if let Some(v) = data.get("global_next_uid").and_then(serde_json::Value::as_u64) {
            state.global_next_uid = v.to_usize();
        }
        if let Some(v) = data.get("cache_hit_tokens").and_then(serde_json::Value::as_u64) {
            state.cache_hit_tokens = v.to_usize();
        }
        if let Some(v) = data.get("cache_miss_tokens").and_then(serde_json::Value::as_u64) {
            state.cache_miss_tokens = v.to_usize();
        }
        if let Some(v) = data.get("total_output_tokens").and_then(serde_json::Value::as_u64) {
            state.total_output_tokens = v.to_usize();
        }
        if let Some(v) = data.get("cost_hit_usd").and_then(serde_json::Value::as_f64) {
            state.cost_hit_usd = v;
        }
        if let Some(v) = data.get("cost_miss_usd").and_then(serde_json::Value::as_f64) {
            state.cost_miss_usd = v;
        }
        if let Some(v) = data.get("cost_output_usd").and_then(serde_json::Value::as_f64) {
            state.cost_output_usd = v;
        }
        if let Some(arr) = data.get("disabled_tools").and_then(|v| v.as_array()) {
            let disabled: Vec<String> = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
            state.tools = crate::modules::active_tool_definitions(&state.active_modules);
            state.tools.push(crate::app::reverie::tools::optimize_context_tool_definition());
            for tool in &mut state.tools {
                if tool.id != "tool_manage" && tool.id != "module_toggle" && disabled.contains(&tool.id) {
                    tool.enabled = false;
                }
            }
        }
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(Kind::OVERVIEW), Kind::new(Kind::TOOLS)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![(Kind::new(Kind::OVERVIEW), "Statistics", false), (Kind::new(Kind::TOOLS), "Configuration", false)]
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("Context", "Manage conversation context and system prompts"),
            ("System", "System configuration and control"),
        ]
    }

    fn context_type_metadata(&self) -> Vec<TypeMeta> {
        vec![
            TypeMeta {
                context_type: "overview",
                icon_id: "overview",
                is_fixed: true,
                needs_cache: false,
                fixed_order: Some(2),
                display_name: "overview",
                short_name: "world",
                needs_async_wait: false,
            },
            TypeMeta {
                context_type: "tools",
                icon_id: "overview",
                is_fixed: true,
                needs_cache: false,
                fixed_order: Some(3),
                display_name: "tools",
                short_name: "tools",
                needs_async_wait: false,
            },
        ]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            Kind::OVERVIEW => Some(Box::new(OverviewPanel)),
            Kind::TOOLS => Some(Box::new(ToolsPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        let mut defs = vec![
            // Context tools
            ToolDefinition::from_yaml("Close_panel", t)
                .short_desc("Remove items from context")
                .category("Context")
                .reverie_allowed(true)
                .param_array("ids", ParamType::String, true)
                .build(),
            // System tools
            ToolDefinition::from_yaml("system_reload", t).short_desc("Restart the TUI").category("System").build(),
            // Meta tools
            ToolDefinition::from_yaml("tool_manage", t)
                .short_desc("Enable/disable tools")
                .category("System")
                .param_array(
                    "changes",
                    ParamType::Object(vec![
                        ToolParam::new("tool", ParamType::String)
                            .desc("Tool ID to change (e.g., 'edit_file', 'glob')")
                            .required(),
                        ToolParam::new("action", ParamType::String)
                            .desc("Action to perform")
                            .enum_vals(&["enable", "disable"])
                            .required(),
                    ]),
                    true,
                )
                .build(),
        ];

        // Panel pagination tool (dynamically enabled/disabled)
        defs.push(
            ToolDefinition::from_yaml("panel_goto_page", t)
                .short_desc("Navigate paginated panel")
                .category("Context")
                .enabled(false)
                .param("panel_id", ParamType::String, true)
                .param("page", ParamType::Integer, true)
                .build(),
        );

        // Add module_toggle tool
        defs.push(super::module_toggle_tool_definition());

        defs
    }

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<Verdict> {
        match tool.name.as_str() {
            "Close_panel" => {
                let mut pf = Verdict::new();
                if let Some(ids) = tool.input.get("ids").and_then(serde_json::Value::as_array) {
                    for id_val in ids {
                        if let Some(id) = id_val.as_str() {
                            if !state.context.iter().any(|c| c.id == id) {
                                pf.warnings.push(format!("Panel '{id}' not found — will be skipped"));
                            } else if state.context.iter().any(|c| c.id == id && c.context_type.is_fixed()) {
                                pf.warnings.push(format!(
                                    "Panel '{id}' is a fixed panel and cannot be closed — will be skipped"
                                ));
                            } else if state.active_modules.contains("tree")
                                && let Some(ctx) =
                                    state.context.iter().find(|c| c.id == id && c.context_type.as_str() == Kind::FILE)
                                && let Some(file_path) = ctx.get_meta_str("file_path")
                                && let Ok(cwd) = std::env::current_dir().and_then(|d| d.canonicalize())
                                && let Ok(rel) = std::path::Path::new(file_path).strip_prefix(&cwd)
                            {
                                let rel_str = rel.to_string_lossy();
                                let ts = cp_mod_tree::types::TreeState::get(state);
                                if let Some(desc) = ts.descriptions.iter().find(|d| d.path == rel_str.as_ref()) {
                                    let current_hash =
                                        cp_mod_tree::tools::compute_file_hash(std::path::Path::new(file_path))
                                            .unwrap_or_default();
                                    if !desc.file_hash.is_empty()
                                        && desc.file_hash != current_hash
                                        && !tools::close_context::has_pending_tree_describe(state, rel_str.as_ref())
                                    {
                                        pf.warnings.push(format!(
                                            "Panel '{id}' ({rel_str}) has a stale [!] tree description — \
                                             will be skipped. Update it with tree_describe first."
                                        ));
                                    }
                                } else if !tools::close_context::has_pending_tree_describe(state, rel_str.as_ref()) {
                                    pf.warnings.push(format!(
                                        "Panel '{id}' ({rel_str}) has no tree description — \
                                         will be skipped. Add one with tree_describe first."
                                    ));
                                }
                            }
                        }
                    }
                }
                Some(pf)
            }
            "tool_manage" => {
                let mut pf = Verdict::new();
                if let Some(changes) = tool.input.get("changes").and_then(serde_json::Value::as_array) {
                    for change in changes {
                        if let Some(tool_id) = change.get("tool").and_then(serde_json::Value::as_str) {
                            if !state.tools.iter().any(|t| t.id == tool_id) {
                                pf.errors.push(format!("Tool '{tool_id}' not found"));
                            } else if tool_id == "tool_manage" {
                                pf.errors.push("Cannot disable 'tool_manage' — it protects itself".to_string());
                            }
                        }
                    }
                }
                Some(pf)
            }
            "panel_goto_page" => {
                let mut pf = Verdict::new();
                if let Some(panel_id) = tool.input.get("panel_id").and_then(serde_json::Value::as_str) {
                    match state.context.iter().find(|c| c.id == panel_id) {
                        None => pf.errors.push(format!("Panel '{panel_id}' not found")),
                        Some(ctx) => {
                            if let Some(page) = tool.input.get("page").and_then(serde_json::Value::as_i64) {
                                let total = ctx.total_pages.to_i64();
                                if total > 0 && (page < 1 || page > total) {
                                    pf.errors.push(format!("Page {page} out of range (1-{total})"));
                                }
                            }
                        }
                    }
                }
                Some(pf)
            }
            _ => None,
        }
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            // Context tools
            "Close_panel" => Some(tools::close_context::execute(tool, state)),
            "panel_goto_page" => Some(tools::panel_goto_page::execute(tool, state)),

            // System tools (reload stays in core)
            "system_reload" => Some(crate::infra::tools::execute_reload_tui(tool, state)),

            // Meta tools
            "tool_manage" => Some(tools::manage_tools::execute(tool, state)),

            // module_toggle is handled in dispatch_tool() directly
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![
            ("Close_panel", visualizers::visualize_core_output),
            ("tool_manage", visualizers::visualize_core_output),
            ("system_reload", visualizers::visualize_core_output),
            ("panel_goto_page", visualizers::visualize_core_output),
        ]
    }

    fn dependencies(&self) -> &[&'static str] {
        &[]
    }

    fn init_state(&self, _state: &mut State) {}

    fn reset_state(&self, _state: &mut State) {}

    fn dynamic_panel_types(&self) -> Vec<Kind> {
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
