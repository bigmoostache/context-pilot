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
use crate::infra::tools::{ToolDefinition, ToolResult, ToolUse};
use crate::state::{Kind, State};

pub(crate) use cp_agora::AgoraModule;
pub(crate) use cp_mod_brave::BraveModule;
pub(crate) use cp_mod_bridge::BridgeModule;
pub(crate) use cp_mod_callback::CallbackModule;
pub(crate) use cp_mod_console::ConsoleModule;
pub(crate) use cp_mod_entities::EntitiesModule;
pub(crate) use cp_mod_files::FilesModule;
pub(crate) use cp_mod_firecrawl::FirecrawlModule;
pub(crate) use cp_mod_git::GitModule;
pub(crate) use cp_mod_github::GithubModule;
pub(crate) use cp_mod_logs::LogsModule;
pub(crate) use cp_mod_memory::MemoryModule;
pub(crate) use cp_mod_ocr::OcrModule;
pub(crate) use cp_mod_prompt::PromptModule;
pub(crate) use cp_mod_queue::QueueModule;
pub(crate) use cp_mod_scratchpad::ScratchpadModule;
pub(crate) use cp_mod_search::SearchModule;
pub(crate) use cp_mod_spine::SpineModule;
pub(crate) use cp_mod_threads::ThreadsModule;
pub(crate) use cp_mod_todo::TodoModule;
pub(crate) use cp_mod_tree::TreeModule;

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
            lookup.get(&ct).map(|entry| FixedPanelDefault {
                module_id: entry.0,
                is_core: entry.1,
                context_type: ct,
                display_name: entry.2,
                cache_deprecated: entry.3,
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
        Box::new(PromptModule::new()),
        Box::new(FilesModule::new()),
        Box::new(TreeModule::new()),
        Box::new(GitModule::new()),
        Box::new(GithubModule::new()),
        Box::new(ConsoleModule::new()),
        Box::new(CallbackModule::new()),
        Box::new(TodoModule::new()),
        Box::new(MemoryModule::new()),
        Box::new(AgoraModule::new()),
        Box::new(OcrModule::new()),
        Box::new(ScratchpadModule::new()),
        Box::new(ThreadsModule::new()),
        Box::new(SpineModule::new()),
        Box::new(LogsModule::new()),
        Box::new(BraveModule::new()),
        Box::new(FirecrawlModule::new()),
        Box::new(QueueModule::new()),
        Box::new(SearchModule::new()),
        Box::new(EntitiesModule::new()),
        Box::new(BridgeModule::new()),
    ]
}

/// Returns the default set of active module IDs (all modules).
pub(crate) fn default_active_modules() -> HashSet<String> {
    all_modules().iter().map(|m| m.id().to_owned()).collect()
}

/// Build a registry of tool visualizers from all modules.
/// Maps `tool_id` -> visualizer function. Used by `conversation_render` to
/// dispatch custom rendering for tool results.
pub(crate) fn build_visualizer_registry() -> HashMap<String, ToolVisualizer> {
    let mut registry = HashMap::new();
    for module in all_modules() {
        for (tool_id, visualizer) in module.tool_visualizers() {
            let _r = registry.insert(tool_id.to_owned(), visualizer);
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
    let _fg = cp_base::flame!(&format!("tool_{}", tool.name));
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
        tldr: None,
        is_error: true,
        preserves_tempo: false,
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
