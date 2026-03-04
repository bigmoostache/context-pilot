//! Typst PDF module — embedded compiler for document generation.
//!
//! Provides `typst_execute` tool (compile, watch, init, query, fonts).
//! Automatically seeds shared templates and registers a file-change callback
//! that recompiles watched documents when any dependency changes.

// Silent callback test
/// Parses typst CLI subcommands into structured arguments.
pub mod cli_parser;
/// Embedded Typst compiler (World implementation, font loading, package resolution).
pub mod compiler;
/// Package management: download and cache `@preview/` packages from Typst Universe.
pub mod packages;
/// Template seeding: copy built-in templates into `.context-pilot/shared/typst-templates/`.
pub mod templates;
mod tools_execute;
/// Watchlist: tracked documents and their full dependency trees for auto-recompilation.
pub mod watchlist;

use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::{ContextType, State};
use cp_base::tools::{ParamType, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> = std::sync::LazyLock::new(|| {
    serde_yaml::from_str(include_str!("../../../yamls/tools/typst.yaml")).expect("Failed to parse typst tool YAML")
});

/// Typst PDF module: embedded compiler, watch mode, template management.
#[derive(Debug, Clone, Copy)]
pub struct TypstModule;

/// Templates live here — in the shared (version-controlled) folder.
pub const TEMPLATES_DIR: &str = ".context-pilot/shared/typst-templates";

impl Module for TypstModule {
    fn id(&self) -> &'static str {
        "typst"
    }

    fn name(&self) -> &'static str {
        "Typst PDF"
    }

    fn description(&self) -> &'static str {
        "PDF generation via embedded Typst compiler"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["core", "callback"]
    }

    fn is_global(&self) -> bool {
        true
    }

    fn init_state(&self, state: &mut State) {
        cp_base::config::constants::ensure_shared_dir();
        ensure_typst_callback(state);
        templates::seed_templates();
    }

    fn reset_state(&self, _state: &mut State) {
        // No state to reset — stateless module
    }

    fn load_module_data(&self, _data: &serde_json::Value, state: &mut State) {
        cp_base::config::constants::ensure_shared_dir();
        ensure_typst_callback(state);
        templates::seed_templates();
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("typst_execute", t)
                .short_desc("Run typst commands via embedded compiler")
                .category("PDF")
                .param("command", ParamType::String, true)
                .build(),
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "typst_execute" => Some(tools_execute::execute_typst(tool, state)),
            _ => None,
        }
    }

    fn create_panel(&self, _context_type: &ContextType) -> Option<Box<dyn Panel>> {
        None
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("PDF", "Create and manage Typst PDF documents")]
    }
}

/// Ensure the typst watchlist callback exists in `CallbackState`.
/// Single callback that watches ALL files (*) and checks against the watchlist's dependency trees.
fn ensure_typst_callback(state: &mut State) {
    use cp_mod_callback::types::{CallbackDefinition, CallbackState};

    let cs = CallbackState::get_mut(state);

    let binary_path = std::env::current_exe().unwrap_or_default().to_string_lossy().to_string();

    // Remove old callbacks from previous designs
    cs.definitions.retain(|d| {
        d.name != "typst-compile"
            && d.name != "typst-compile-template"
            && d.name != "typst-template-recompile"
            && d.name != "typst-watchlist"
    });
    cs.active_set.retain(|id| cs.definitions.iter().any(|d| &d.id == id));

    // Single callback: watches ALL files, checks watchlist to find affected docs
    let cb_id = format!("CB{}", cs.next_id);
    cs.next_id += 1;

    // The CLI subcommand reads the watchlist, checks if any changed files are dependencies,
    // and recompiles affected documents (updating deps at the same time).
    let script = format!(r"bash -c '{binary_path} typst-recompile-watched $CP_CHANGED_FILES'");

    cs.definitions.push(CallbackDefinition {
        id: cb_id.clone(),
        name: "typst-watchlist".to_string(),
        description: "Recompile watched .typ documents when their dependencies change".to_string(),
        pattern: "*".to_string(),
        blocking: true,
        timeout_secs: Some(60),
        success_message: None,
        cwd: None,
        one_at_a_time: false,
        built_in: true,
        built_in_command: Some(script),
    });
    let _ = cs.active_set.insert(cb_id);
}
