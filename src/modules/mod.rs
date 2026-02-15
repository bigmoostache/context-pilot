pub mod core;

use std::collections::HashSet;

use crate::core::panels::Panel;
use crate::state::{ContextType, State};
use crate::tool_defs::{ParamType, ToolDefinition, ToolParam};
use crate::tools::{ToolResult, ToolUse};

pub use cp_mod_files::FilesModule;
pub use cp_mod_git::GitModule;
pub use cp_mod_github::GithubModule;
pub use cp_mod_glob::GlobModule;
pub use cp_mod_grep::GrepModule;
pub use cp_mod_logs::LogsModule;
pub use cp_mod_memory::MemoryModule;
pub use cp_mod_preset::PresetModule;
pub use cp_mod_prompt::PromptModule;
pub use cp_mod_scratchpad::ScratchpadModule;
pub use cp_mod_spine::SpineModule;
pub use cp_mod_tmux::TmuxModule;
pub use cp_mod_todo::TodoModule;
pub use cp_mod_tree::TreeModule;

// Re-export Module trait and helpers from cp-base
pub use cp_base::modules::{Module, run_with_timeout, truncate_output};

/// Canonical order of fixed panels.
/// This defines the sidebar position and ID assignment for each fixed panel.
/// Conversation is NOT included — it's the live chat feed, not a numbered panel.
/// System (Seed) is NOT included — agent prompt is injected as system message.
const FIXED_PANEL_ORDER: &[&str] = &[
    ContextType::TODO,       // P1
    ContextType::LIBRARY,    // P2
    ContextType::OVERVIEW,   // P3
    ContextType::TREE,       // P4
    ContextType::MEMORY,     // P5
    ContextType::SPINE,      // P6
    ContextType::LOGS,       // P7
    ContextType::GIT,        // P8
    ContextType::SCRATCHPAD, // P9
];

/// Collect all fixed panel defaults in canonical P0-P7 order.
/// Returns (module_id, is_core, context_type, display_name, cache_deprecated) for each fixed panel.
pub fn all_fixed_panel_defaults() -> Vec<(&'static str, bool, ContextType, &'static str, bool)> {
    // Build a lookup from context_type to module defaults
    let modules = all_modules();
    let mut lookup: std::collections::HashMap<ContextType, (&str, bool, &str, bool)> = std::collections::HashMap::new();
    for module in &modules {
        for (ct, name, cache_dep) in module.fixed_panel_defaults() {
            lookup.insert(ct, (module.id(), module.is_core(), name, cache_dep));
        }
    }

    // Return in canonical order
    FIXED_PANEL_ORDER
        .iter()
        .filter_map(|ct_str| {
            let ct = ContextType::new(ct_str);
            lookup.get(&ct).map(|(mid, is_core, name, cache_dep)| (*mid, *is_core, ct, *name, *cache_dep))
        })
        .collect()
}

/// Create a default ContextElement for a fixed panel
pub fn make_default_context_element(
    id: &str,
    context_type: ContextType,
    name: &str,
    cache_deprecated: bool,
) -> crate::state::ContextElement {
    cp_base::state::make_default_context_element(id, context_type, name, cache_deprecated)
}

/// Returns all registered modules.
pub fn all_modules() -> Vec<Box<dyn Module>> {
    vec![
        Box::new(core::CoreModule),
        Box::new(PromptModule),
        Box::new(FilesModule),
        Box::new(TreeModule),
        Box::new(GitModule),
        Box::new(GithubModule),
        Box::new(GlobModule),
        Box::new(GrepModule),
        Box::new(TmuxModule),
        Box::new(TodoModule),
        Box::new(MemoryModule),
        Box::new(ScratchpadModule),
        Box::new(PresetModule::new(all_modules, active_tool_definitions, crate::core::ensure_default_contexts)),
        Box::new(SpineModule),
        Box::new(LogsModule),
    ]
}

/// Returns the default set of active module IDs (all modules).
pub fn default_active_modules() -> HashSet<String> {
    all_modules().iter().map(|m| m.id().to_string()).collect()
}

/// Collect tool definitions from all active modules.
pub fn active_tool_definitions(active_modules: &HashSet<String>) -> Vec<ToolDefinition> {
    all_modules().into_iter().filter(|m| active_modules.contains(m.id())).flat_map(|m| m.tool_definitions()).collect()
}

/// Dispatch a tool call to the appropriate active module.
pub fn dispatch_tool(tool: &ToolUse, state: &mut State, active_modules: &HashSet<String>) -> ToolResult {
    // Handle module_toggle specially — it's always available when core is active
    if tool.name == "module_toggle" && active_modules.contains("core") {
        return execute_module_toggle(tool, state);
    }

    for module in all_modules() {
        if active_modules.contains(module.id())
            && let Some(result) = module.execute_tool(tool, state)
        {
            return result;
        }
    }

    ToolResult { tool_use_id: tool.id.clone(), content: format!("Unknown tool: {}", tool.name), is_error: true }
}

/// Create a panel for the given context type by asking all modules.
pub fn create_panel(context_type: &ContextType) -> Option<Box<dyn Panel>> {
    for module in all_modules() {
        if let Some(panel) = module.create_panel(context_type) {
            return Some(panel);
        }
    }
    None
}

/// Validate that all dependencies of active modules are also active.
/// Called at startup. Panics on unmet dependencies.
pub fn validate_dependencies(active: &HashSet<String>) {
    for module in all_modules() {
        if active.contains(module.id()) {
            for dep in module.dependencies() {
                if !active.contains(*dep) {
                    panic!("Module '{}' depends on '{}', but '{}' is not active", module.id(), dep, dep);
                }
            }
        }
    }
}

/// Check if a module can be deactivated without breaking dependencies.
/// Returns Ok(()) if safe, Err(message) if blocked.
pub fn check_can_deactivate(id: &str, active: &HashSet<String>) -> Result<(), String> {
    // Core modules cannot be deactivated
    for module in all_modules() {
        if module.id() == id && module.is_core() {
            return Err(format!("Cannot deactivate core module '{}'", id));
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

/// Returns the module_toggle tool definition (added by core module).
pub fn module_toggle_tool_definition() -> ToolDefinition {
    ToolDefinition {
        id: "module_toggle".to_string(),
        name: "Toggle Module".to_string(),
        short_desc: "Activate/deactivate modules".to_string(),
        description: "Activates or deactivates modules. Core module cannot be deactivated. \
            Deactivating a module removes its tools and panels. \
            Cannot deactivate a module if another active module depends on it."
            .to_string(),
        params: vec![
            ToolParam::new(
                "changes",
                ParamType::Array(Box::new(ParamType::Object(vec![
                    ToolParam::new("module", ParamType::String)
                        .desc("Module ID (e.g., 'git', 'memory', 'tmux')")
                        .required(),
                    ToolParam::new("action", ParamType::String)
                        .desc("Action to perform")
                        .enum_vals(&["activate", "deactivate"])
                        .required(),
                ]))),
            )
            .desc("Array of module changes")
            .required(),
        ],
        enabled: true,
        category: "System".to_string(),
    }
}

/// Execute the module_toggle tool.
fn execute_module_toggle(tool: &ToolUse, state: &mut State) -> ToolResult {
    let changes = match tool.input.get("changes").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'changes' parameter (expected array)".to_string(),
                is_error: true,
            };
        }
    };

    let mut successes = Vec::new();
    let mut failures = Vec::new();

    let all_mods = all_modules();
    let known_ids: HashSet<&str> = all_mods.iter().map(|m| m.id()).collect();

    for (i, change) in changes.iter().enumerate() {
        let module_id = match change.get("module").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => {
                failures.push(format!("Change {}: missing 'module' field", i + 1));
                continue;
            }
        };

        let action = match change.get("action").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => {
                failures.push(format!("Change {}: missing 'action' field", i + 1));
                continue;
            }
        };

        if !known_ids.contains(module_id) {
            failures.push(format!("Change {}: unknown module '{}'", i + 1, module_id));
            continue;
        }

        match action {
            "activate" => {
                if state.active_modules.contains(module_id) {
                    successes.push(format!("'{}' already active", module_id));
                } else {
                    state.active_modules.insert(module_id.to_string());
                    // Rebuild tools list
                    rebuild_tools(state);
                    let module = all_mods
                        .iter()
                        .find(|m| m.id() == module_id)
                        .expect("module_id was validated against known_ids but not found");
                    successes.push(format!("activated '{}' ({})", module.name(), module.description()));
                }
            }
            "deactivate" => {
                if !state.active_modules.contains(module_id) {
                    successes.push(format!("'{}' already inactive", module_id));
                } else {
                    match check_can_deactivate(module_id, &state.active_modules) {
                        Ok(()) => {
                            // Find panel types to remove
                            let module = all_mods
                                .iter()
                                .find(|m| m.id() == module_id)
                                .expect("module_id was validated against known_ids but not found");
                            let fixed_types = module.fixed_panel_types();
                            let dynamic_types = module.dynamic_panel_types();

                            // Remove panels owned by this module
                            state.context.retain(|ctx| {
                                !fixed_types.contains(&ctx.context_type) && !dynamic_types.contains(&ctx.context_type)
                            });

                            state.active_modules.remove(module_id);
                            // Rebuild tools list
                            rebuild_tools(state);
                            successes.push(format!("deactivated '{}'", module_id));
                        }
                        Err(msg) => {
                            failures.push(format!("Change {}: {}", i + 1, msg));
                        }
                    }
                }
            }
            _ => {
                failures.push(format!(
                    "Change {}: invalid action '{}' (use 'activate' or 'deactivate')",
                    i + 1,
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
        is_error: !failures.is_empty() && successes.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_active() -> HashSet<String> {
        default_active_modules()
    }

    #[test]
    fn cannot_deactivate_core_module() {
        let active = all_active();
        let result = check_can_deactivate("core", &active);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("core"));
    }

    #[test]
    fn cannot_deactivate_with_dependent() {
        // github depends on git — deactivating git while github is active should fail
        let active = all_active();
        let result = check_can_deactivate("git", &active);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("github"));
    }

    #[test]
    fn can_deactivate_independent_module() {
        let active = all_active();
        // tmux has no dependents
        let result = check_can_deactivate("tmux", &active);
        assert!(result.is_ok());
    }

    #[test]
    fn can_deactivate_when_dependent_inactive() {
        // git can be deactivated if github is not active
        let mut active = all_active();
        active.remove("github");
        let result = check_can_deactivate("git", &active);
        assert!(result.is_ok());
    }
}

/// Rebuild the tools list from active modules and preserved disabled_tools.
fn rebuild_tools(state: &mut State) {
    // Preserve currently disabled tool IDs
    let disabled: HashSet<String> = state.tools.iter().filter(|t| !t.enabled).map(|t| t.id.clone()).collect();

    // Get fresh tool definitions from active modules
    let mut tools = active_tool_definitions(&state.active_modules);

    // Re-apply disabled state
    for tool in &mut tools {
        if tool.id != "tool_manage" && tool.id != "module_toggle" && disabled.contains(&tool.id) {
            tool.enabled = false;
        }
    }

    state.tools = tools;
}
