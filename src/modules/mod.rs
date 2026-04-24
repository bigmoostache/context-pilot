/// Conversation display, input rendering, and message formatting.
pub(crate) mod conversation;
/// Frozen conversation history chunks for context management.
pub(crate) mod conversation_history;
/// Overview panel with token usage, statistics, and configuration.
pub(crate) mod overview;
/// Pre-flight validation for tool calls.
pub(crate) mod pre_flight;
/// Interactive user question forms.
pub(crate) mod questions;

use std::collections::{HashMap, HashSet};

use crate::app::panels::Panel;
use crate::infra::tools::{ParamType, ToolDefinition, ToolParam, ToolTexts};
use crate::infra::tools::{ToolResult, ToolUse};
use crate::state::{Kind, State};

/// Lazily parsed tool text definitions for core tools.
static CORE_TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../yamls/tools/core.yaml")));

pub(crate) use cp_mod_brave::BraveModule;
pub(crate) use cp_mod_callback::CallbackModule;
pub(crate) use cp_mod_chat::ChatModule;
pub(crate) use cp_mod_console::ConsoleModule;
pub(crate) use cp_mod_files::FilesModule;
pub(crate) use cp_mod_firecrawl::FirecrawlModule;
pub(crate) use cp_mod_git::GitModule;
pub(crate) use cp_mod_github::GithubModule;
pub(crate) use cp_mod_logs::LogsModule;
pub(crate) use cp_mod_memory::MemoryModule;
pub(crate) use cp_mod_preset::PresetModule;
pub(crate) use cp_mod_prompt::PromptModule;
pub(crate) use cp_mod_queue::QueueModule;
pub(crate) use cp_mod_scratchpad::ScratchpadModule;
pub(crate) use cp_mod_spine::SpineModule;
pub(crate) use cp_mod_todo::TodoModule;
pub(crate) use cp_mod_tree::TreeModule;
pub(crate) use cp_mod_typst::TypstModule;

// Re-export Module trait and helpers from cp-base
pub(crate) use cp_base::modules::{Module, ToolVisualizer};

/// Initialize the global `Kind` registry from all modules.
/// Must be called once at startup, before any `is_fixed()` / `icon()` / `needs_cache()` calls.
pub(crate) fn init_registry() {
    let modules = all_modules();
    let metadata: Vec<crate::state::TypeMeta> = modules.iter().flat_map(|m| m.context_type_metadata()).collect();
    crate::state::init_context_type_registry(metadata);
}

/// Metadata for a fixed panel default.
pub(crate) struct FixedPanelDefault {
    /// Unique identifier of the owning module.
    pub module_id: &'static str,
    /// Whether this module is a core (non-deactivatable) module.
    pub is_core: bool,
    /// The context type of this fixed panel.
    pub context_type: Kind,
    /// Human-readable display name for the panel.
    pub display_name: &'static str,
    /// Whether the cache for this panel is deprecated.
    pub cache_deprecated: bool,
}

/// Lookup entry for fixed panel defaults: (`module_id`, `is_core`, `display_name`, `cache_deprecated`).
type FixedPanelLookup<'lookup> = (&'lookup str, bool, &'lookup str, bool);

/// Collect all fixed panel defaults in canonical order (derived from the registry).
pub(crate) fn all_fixed_panel_defaults() -> Vec<FixedPanelDefault> {
    // Build a lookup from context_type to module defaults
    let modules = all_modules();
    let mut lookup: HashMap<Kind, FixedPanelLookup<'_>> = HashMap::new();
    for module in &modules {
        for (ct, name, cache_dep) in module.fixed_panel_defaults() {
            let _r = lookup.insert(ct, (module.id(), module.is_core(), name, cache_dep));
        }
    }

    // Return in canonical order (derived from registry metadata)
    crate::state::fixed_panel_order()
        .iter()
        .filter_map(|ct_str| {
            let ct = Kind::new(ct_str);
            lookup.get(&ct).map(|(mid, is_core, name, cache_dep)| FixedPanelDefault {
                module_id: mid,
                is_core: *is_core,
                context_type: ct,
                display_name: name,
                cache_deprecated: *cache_dep,
            })
        })
        .collect()
}

/// Create a default `Entry` for a fixed panel
pub(crate) fn make_default_entry(
    id: &str,
    context_type: Kind,
    name: &str,
    cache_deprecated: bool,
) -> crate::state::Entry {
    cp_base::state::context::make_default_entry(id, context_type, name, cache_deprecated)
}

/// Returns all registered modules.
pub(crate) fn all_modules() -> Vec<Box<dyn Module>> {
    vec![
        Box::new(overview::OverviewModule),
        Box::new(conversation::ConversationModule),
        Box::new(conversation_history::ConversationHistoryModule),
        Box::new(questions::QuestionsModule),
        Box::new(PromptModule),
        Box::new(FilesModule),
        Box::new(TreeModule),
        Box::new(GitModule),
        Box::new(GithubModule),
        Box::new(ConsoleModule),
        Box::new(CallbackModule),
        Box::new(TodoModule),
        Box::new(MemoryModule),
        Box::new(ScratchpadModule),
        Box::new(PresetModule::new(all_modules, active_tool_definitions, crate::app::ensure_default_contexts)),
        Box::new(SpineModule),
        Box::new(LogsModule),
        Box::new(TypstModule),
        Box::new(BraveModule),
        Box::new(FirecrawlModule),
        Box::new(QueueModule),
        Box::new(ChatModule),
    ]
}

/// Returns the default set of active module IDs (all modules).
pub(crate) fn default_active_modules() -> HashSet<String> {
    all_modules().iter().map(|m| m.id().to_string()).collect()
}

/// Build a registry of tool visualizers from all modules.
/// Maps `tool_id` -> visualizer function. Used by `conversation_render` to
/// dispatch custom rendering for tool results.
pub(crate) fn build_visualizer_registry() -> HashMap<String, ToolVisualizer> {
    let mut registry = HashMap::new();
    for module in all_modules() {
        for (tool_id, visualizer) in module.tool_visualizers() {
            let _r = registry.insert(tool_id.to_string(), visualizer);
        }
    }
    registry
}

/// Collect tool definitions from all active modules.
pub(crate) fn active_tool_definitions(active_modules: &HashSet<String>) -> Vec<ToolDefinition> {
    all_modules().into_iter().filter(|m| active_modules.contains(m.id())).flat_map(|m| m.tool_definitions()).collect()
}

/// Dispatch a tool call to the appropriate active module.
pub(crate) fn dispatch_tool(tool: &ToolUse, state: &mut State, active_modules: &HashSet<String>) -> ToolResult {
    // Handle module_toggle specially — it's always available when core is active
    if tool.name == "module_toggle" && active_modules.contains("core") {
        return execute_module_toggle(tool, state);
    }

    // Handle reverie tools — optimize_context for main AI, report + allowed tools for reverie
    if tool.name == "optimize_context" {
        return crate::app::reverie::tools::execute_optimize_context(tool, state);
    }

    for module in all_modules() {
        if active_modules.contains(module.id())
            && let Some(mut result) = module.execute_tool(tool, state)
        {
            // Ensure tool_name is set for visualization dispatch
            result.tool_name.clone_from(&tool.name);
            return result;
        }
    }

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Unknown tool: {}", tool.name),
        display: None,
        is_error: true,
        tool_name: tool.name.clone(),
    }
}

/// Create a panel for the given context type by asking all modules.
pub(crate) fn create_panel(context_type: &Kind) -> Option<Box<dyn Panel>> {
    for module in all_modules() {
        if let Some(panel) = module.create_panel(context_type) {
            return Some(panel);
        }
    }
    None
}

/// Validate that all active module dependencies are satisfied.
pub(crate) fn validate_dependencies(active: &HashSet<String>) {
    for module in all_modules() {
        if active.contains(module.id()) {
            for dep in module.dependencies() {
                assert!(
                    active.contains(*dep),
                    "Module '{}' depends on '{}', but '{}' is not active",
                    module.id(),
                    dep,
                    dep
                );
            }
        }
    }
}

/// Check if a module can be deactivated without breaking dependencies.
/// Returns Ok(()) if safe, Err(message) if blocked.
pub(crate) fn check_can_deactivate(id: &str, active: &HashSet<String>) -> Result<(), String> {
    // Core modules cannot be deactivated
    for module in all_modules() {
        if module.id() == id && module.is_core() {
            return Err(format!("Cannot deactivate core module '{id}'"));
        }
    }

    // Check if any other active module depends on this one
    for module in all_modules() {
        if module.id() != id && active.contains(module.id()) && module.dependencies().contains(&id) {
            return Err(format!("Cannot deactivate '{}': required by '{}'", id, module.id()));
        }
    }

    Ok(())
}

/// Returns the `module_toggle` tool definition (added by core module).
pub(crate) fn module_toggle_tool_definition() -> ToolDefinition {
    let t = &*CORE_TOOL_TEXTS;
    ToolDefinition::from_yaml("module_toggle", t)
        .short_desc("Activate/deactivate modules")
        .category("System")
        .param_array(
            "changes",
            ParamType::Object(vec![
                ToolParam::new("module", ParamType::String)
                    .desc("Module ID (e.g., 'git', 'memory', 'tmux')")
                    .required(),
                ToolParam::new("action", ParamType::String)
                    .desc("Action to perform")
                    .enum_vals(&["activate", "deactivate"])
                    .required(),
            ]),
            true,
        )
        .build()
}

/// Execute the `module_toggle` tool.
fn execute_module_toggle(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(changes) = tool.input.get("changes").and_then(serde_json::Value::as_array) else {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "Missing 'changes' parameter (expected array)".to_string(),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
        };
    };

    let mut successes = Vec::new();
    let mut failures = Vec::new();

    let all_mods = all_modules();
    let known_ids: HashSet<&str> = all_mods.iter().map(|m| m.id()).collect();

    for (i, change) in changes.iter().enumerate() {
        let Some(module_id) = change.get("module").and_then(serde_json::Value::as_str) else {
            failures.push(format!("Change {}: missing 'module' field", i.saturating_add(1)));
            continue;
        };

        let Some(action) = change.get("action").and_then(serde_json::Value::as_str) else {
            failures.push(format!("Change {}: missing 'action' field", i.saturating_add(1)));
            continue;
        };

        if !known_ids.contains(module_id) {
            failures.push(format!("Change {}: unknown module '{}'", i.saturating_add(1), module_id));
            continue;
        }

        match action {
            "activate" => {
                if state.active_modules.contains(module_id) {
                    successes.push(format!("'{module_id}' already active"));
                } else {
                    let _r = state.active_modules.insert(module_id.to_string());
                    // Rebuild tools list
                    rebuild_tools(state);
                    let description = all_mods
                        .iter()
                        .find(|m| m.id() == module_id)
                        .map_or_else(|| "unknown".to_string(), |m| format!("'{}' ({})", m.name(), m.description()));
                    successes.push(format!("activated {description}"));
                }
            }
            "deactivate" => {
                if state.active_modules.contains(module_id) {
                    match check_can_deactivate(module_id, &state.active_modules) {
                        Ok(()) => {
                            // Find panel types to remove
                            let (fixed_types, dynamic_types) =
                                all_mods.iter().find(|m| m.id() == module_id).map_or_else(
                                    || (Vec::new(), Vec::new()),
                                    |m| (m.fixed_panel_types(), m.dynamic_panel_types()),
                                );

                            // Remove panels owned by this module
                            state.context.retain(|ctx| {
                                !fixed_types.contains(&ctx.context_type) && !dynamic_types.contains(&ctx.context_type)
                            });

                            let _r = state.active_modules.remove(module_id);
                            // Rebuild tools list
                            rebuild_tools(state);
                            successes.push(format!("deactivated '{module_id}'"));
                        }
                        Err(msg) => {
                            failures.push(format!("Change {}: {}", i.saturating_add(1), msg));
                        }
                    }
                } else {
                    successes.push(format!("'{module_id}' already inactive"));
                }
            }
            _ => {
                failures.push(format!(
                    "Change {}: invalid action '{}' (use 'activate' or 'deactivate')",
                    i.saturating_add(1),
                    action
                ));
            }
        }
    }

    let mut result_parts = Vec::new();
    if !successes.is_empty() {
        result_parts.push(format!("OK: {}", successes.join(", ")));
    }
    if !failures.is_empty() {
        result_parts.push(format!("FAILED: {}", failures.join("; ")));
    }

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: result_parts.join("\n"),
        display: None,
        is_error: !failures.is_empty() && successes.is_empty(),
        tool_name: tool.name.clone(),
    }
}

/// Rebuild the tools list from active modules and preserved `disabled_tools`.
fn rebuild_tools(state: &mut State) {
    // Preserve currently disabled tool IDs
    let disabled: HashSet<String> = state.tools.iter().filter(|t| !t.enabled).map(|t| t.id.clone()).collect();

    // Get fresh tool definitions from active modules
    let mut tools = active_tool_definitions(&state.active_modules);

    // Add the reverie's optimize_context tool (always available for main AI)
    tools.push(crate::app::reverie::tools::optimize_context_tool_definition());

    // Re-apply disabled state
    for tool in &mut tools {
        if tool.id != "tool_manage" && tool.id != "module_toggle" && disabled.contains(&tool.id) {
            tool.enabled = false;
        }
    }

    state.tools = tools;
}
