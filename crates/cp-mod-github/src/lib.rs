pub(crate) mod cache_invalidation;
pub mod classify;
mod panel;
mod tools;

use cp_base::panels::Panel;
use cp_base::state::{ContextType, State};
use cp_base::tool_defs::{ParamType, ToolDefinition, ToolParam};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::GithubResultPanel;
use cp_base::modules::Module;

pub struct GithubModule;

impl Module for GithubModule {
    fn id(&self) -> &'static str {
        "github"
    }
    fn name(&self) -> &'static str {
        "GitHub"
    }
    fn description(&self) -> &'static str {
        "GitHub API operations via gh CLI"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["git"]
    }

    fn dynamic_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::new(ContextType::GITHUB_RESULT)]
    }

    fn create_panel(&self, context_type: &ContextType) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            ContextType::GITHUB_RESULT => Some(Box::new(GithubResultPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            id: "gh_execute".to_string(),
            name: "GitHub Execute".to_string(),
            short_desc: "Run gh commands".to_string(),
            description: "Executes a GitHub CLI (gh) command. Requires GITHUB_TOKEN in environment. \
                    Read-only commands (pr list, issue view, etc.) create a dynamic result panel that \
                    auto-refreshes every 120 seconds. Mutating commands (pr create, issue close, etc.) \
                    execute directly and return output. Shell operators (|, ;, &&) are not allowed."
                .to_string(),
            params: vec![
                ToolParam::new("command", ParamType::String)
                    .desc("Full gh command string (e.g., 'gh pr list', 'gh issue view 42')")
                    .required(),
            ],
            enabled: true,
            category: "GitHub".to_string(),
        }]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "gh_execute" => Some(self::tools::execute_gh_command(tool, state)),
            _ => None,
        }
    }
}
