pub mod cache_invalidation;
pub mod classify;
mod panel;
pub(crate) mod tools;
pub mod types;

/// Refresh interval for git status (milliseconds)
pub const GIT_STATUS_REFRESH_MS: u64 = 2_000; // 2 seconds

use serde_json::json;

use crate::core::panels::Panel;
use crate::state::{ContextType, State};
use crate::tool_defs::{ParamType, ToolCategory, ToolDefinition, ToolParam};
use crate::tools::{ToolResult, ToolUse};

use self::panel::{GitPanel, GitResultPanel};
use super::Module;

pub struct GitModule;

impl Module for GitModule {
    fn id(&self) -> &'static str {
        "git"
    }
    fn name(&self) -> &'static str {
        "Git"
    }
    fn description(&self) -> &'static str {
        "Git version control tools and status panel"
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        json!({
            "git_show_diffs": state.git_show_diffs,
            "git_diff_base": state.git_diff_base,
        })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(v) = data.get("git_show_diffs").and_then(|v| v.as_bool()) {
            state.git_show_diffs = v;
        }
        if let Some(v) = data.get("git_diff_base").and_then(|v| v.as_str()) {
            state.git_diff_base = Some(v.to_string());
        }
    }

    fn fixed_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::Git]
    }

    fn dynamic_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::GitResult]
    }

    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> {
        vec![(ContextType::Git, "Changes", false)]
    }

    fn create_panel(&self, context_type: ContextType) -> Option<Box<dyn Panel>> {
        match context_type {
            ContextType::Git => Some(Box::new(GitPanel)),
            ContextType::GitResult => Some(Box::new(GitResultPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                id: "git_execute".to_string(),
                name: "Git Execute".to_string(),
                short_desc: "Run git commands".to_string(),
                description: "Executes a git command. Read-only commands (log, diff, show, status, blame, etc.) \
                    create a dynamic result panel that auto-refreshes. Mutating commands (commit, push, pull, \
                    merge, rebase, etc.) execute directly and return output. Shell operators (|, ;, &&) are \
                    not allowed."
                    .to_string(),
                params: vec![
                    ToolParam::new("command", ParamType::String)
                        .desc("Full git command string (e.g., 'git log --oneline -10', 'git commit -m \"message\"')")
                        .required(),
                ],
                enabled: true,
                category: ToolCategory::Git,
            },
            ToolDefinition {
                id: "git_configure_p6".to_string(),
                name: "Configure Git Panel".to_string(),
                short_desc: "Configure P6 panel".to_string(),
                description: "Configures the P6 git status panel. Can toggle diff display, log display, \
                    change log arguments, and set a diff base ref for comparison."
                    .to_string(),
                params: vec![
                    ToolParam::new("show_diffs", ParamType::Boolean).desc("Show full diff content in P6 panel"),
                    ToolParam::new("show_logs", ParamType::Boolean).desc("Show recent commit history in P6 panel"),
                    ToolParam::new("log_args", ParamType::String)
                        .desc("Custom git log arguments (e.g., '-10 --oneline')"),
                    ToolParam::new("diff_base", ParamType::String)
                        .desc("Git ref to diff against (e.g., 'HEAD~3', 'main'). Set to empty string to clear."),
                ],
                enabled: true,
                category: ToolCategory::Git,
            },
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "git_execute" => Some(self::tools::execute_git_command(tool, state)),
            "git_configure_p6" => Some(self::tools::execute_configure_p6(tool, state)),
            _ => None,
        }
    }
}
