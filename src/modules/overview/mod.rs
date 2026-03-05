pub(crate) mod context;
mod panel;
mod render;
mod render_details;
mod tools;
mod tools_panel;

use serde_json::json;

use crate::app::panels::Panel;
use crate::infra::tools::{ParamType, PreFlightResult, ToolDefinition, ToolParam, ToolTexts};
use crate::infra::tools::{ToolResult, ToolUse};
use crate::modules::ToolVisualizer;
use crate::state::{ContextType, ContextTypeMeta, State};

use self::panel::OverviewPanel;
use self::tools_panel::ToolsPanel;
use super::Module;
use cp_base::cast::SafeCast;

static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/core.yaml")));

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
            "secondary_provider": state.secondary_provider,
            "secondary_anthropic_model": state.secondary_anthropic_model,
            "secondary_grok_model": state.secondary_grok_model,
            "secondary_groq_model": state.secondary_groq_model,
            "secondary_deepseek_model": state.secondary_deepseek_model,
            "reverie_enabled": state.flags.config.reverie_enabled,
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
            state.active_modules = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
            // Auto-add newly introduced modules that aren't in the persisted config.
            // This handles the case where a new module is added to the codebase but
            // the user's config.json was written before it existed.
            let all_defaults = crate::modules::default_active_modules();
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
        if let Some(v) = data.get("reverie_enabled").and_then(serde_json::Value::as_bool) {
            state.flags.config.reverie_enabled = v;
        }
        if let Some(v) = data.get("cleaning_threshold").and_then(serde_json::Value::as_f64) {
            state.cleaning_threshold = v.to_f32();
        }
        if let Some(v) = data.get("cleaning_target_proportion").and_then(serde_json::Value::as_f64) {
            state.cleaning_target_proportion = v.to_f32();
        }
        if let Some(v) = data.get("context_budget") {
            state.context_budget = v.as_u64().map(SafeCast::to_usize);
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
        if let Some(arr) = data.get("disabled_tools").and_then(|v| v.as_array()) {
            let disabled: Vec<String> = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
            // Build tools from active_modules (must be loaded already) and apply disabled state
            state.tools = crate::modules::active_tool_definitions(&state.active_modules);
            // Add reverie's optimize_context tool (always available for main AI)
            state.tools.push(crate::app::reverie::tools::optimize_context_tool_definition());
            for tool in &mut state.tools {
                if tool.id != "tool_manage" && tool.id != "module_toggle" && disabled.contains(&tool.id) {
                    tool.enabled = false;
                }
            }
        }
    }

    fn fixed_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::new(ContextType::OVERVIEW), ContextType::new(ContextType::TOOLS)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> {
        vec![
            (ContextType::new(ContextType::OVERVIEW), "Statistics", false),
            (ContextType::new(ContextType::TOOLS), "Configuration", false),
        ]
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("Context", "Manage conversation context and system prompts"),
            ("System", "System configuration and control"),
        ]
    }

    fn context_type_metadata(&self) -> Vec<ContextTypeMeta> {
        vec![
            ContextTypeMeta {
                context_type: "overview",
                icon_id: "overview",
                is_fixed: true,
                needs_cache: false,
                fixed_order: Some(2),
                display_name: "overview",
                short_name: "world",
                needs_async_wait: false,
            },
            ContextTypeMeta {
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

    fn create_panel(&self, context_type: &ContextType) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            ContextType::OVERVIEW => Some(Box::new(OverviewPanel)),
            ContextType::TOOLS => Some(Box::new(ToolsPanel)),
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

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<PreFlightResult> {
        match tool.name.as_str() {
            "Close_panel" => {
                let mut pf = PreFlightResult::new();
                if let Some(ids) = tool.input.get("ids").and_then(|v| v.as_array()) {
                    for id_val in ids {
                        if let Some(id) = id_val.as_str() {
                            if !state.context.iter().any(|c| c.id == id) {
                                pf.warnings.push(format!("Panel '{id}' not found — will be skipped"));
                            } else if state.context.iter().any(|c| c.id == id && c.context_type.is_fixed()) {
                                pf.warnings.push(format!(
                                    "Panel '{id}' is a fixed panel and cannot be closed — will be skipped"
                                ));
                            }
                        }
                    }
                }
                Some(pf)
            }
            "tool_manage" => {
                let mut pf = PreFlightResult::new();
                if let Some(changes) = tool.input.get("changes").and_then(|v| v.as_array()) {
                    for change in changes {
                        if let Some(tool_id) = change.get("tool").and_then(|v| v.as_str()) {
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
                let mut pf = PreFlightResult::new();
                if let Some(panel_id) = tool.input.get("panel_id").and_then(|v| v.as_str()) {
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
            ("Close_panel", visualize_core_output),
            ("tool_manage", visualize_core_output),
            ("system_reload", visualize_core_output),
            ("panel_goto_page", visualize_core_output),
        ]
    }
}

/// Visualizer for core tool results.
/// Colors closed panel names, highlights enabled/disabled tool status, and shows module activation state changes.
fn visualize_core_output(content: &str, width: usize) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::prelude::*;

    let success_color = Color::Rgb(80, 250, 123);
    let info_color = Color::Rgb(139, 233, 253);
    let warning_color = Color::Rgb(241, 250, 140);
    let error_color = Color::Rgb(255, 85, 85);

    let mut lines = Vec::new();

    for line in content.lines() {
        if line.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        let style = if line.starts_with("Error:") {
            Style::default().fg(error_color)
        } else if line.starts_with("Closed") || line.contains("enabled") {
            Style::default().fg(success_color)
        } else if line.contains("disabled") {
            Style::default().fg(warning_color)
        } else if line.contains("Reloading") || line.contains("TUI") {
            Style::default().fg(info_color)
        } else if line.starts_with("P") && line.chars().nth(1).is_some_and(|c| c.is_ascii_digit()) {
            // Panel IDs like P1, P2
            Style::default().fg(info_color)
        } else {
            Style::default()
        };

        let display = if line.len() > width {
            format!("{}...", &line.get(..line.floor_char_boundary(width.saturating_sub(3))).unwrap_or(""))
        } else {
            line.to_string()
        };
        lines.push(Line::from(Span::styled(display, style)));
    }

    lines
}
