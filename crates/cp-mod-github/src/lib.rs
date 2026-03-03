pub(crate) mod cache_invalidation;
pub mod classify;
mod panel;
pub mod parse;
mod tools;
pub mod types;
pub mod watcher;

pub use types::GithubState;

/// Timeout for gh commands (seconds)
pub const GH_CMD_TIMEOUT_SECS: u64 = 60;

use cp_base::modules::ToolVisualizer;
use cp_base::panels::Panel;
use cp_base::state::{ContextType, State};
use cp_base::tools::{ParamType, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::GithubResultPanel;
use cp_base::modules::Module;

static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> = std::sync::LazyLock::new(|| {
    serde_yaml::from_str(include_str!("../../../yamls/tools/github.yaml")).expect("Failed to parse github tool YAML")
});

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
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("gh_execute", t)
                .short_desc("Run gh commands")
                .category("Git")
                .param("command", ParamType::String, true)
                .build(),
        ]
    }

    fn init_state(&self, state: &mut State) {
        state.set_ext(GithubState::new());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(GithubState::new());
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "gh_execute" => Some(self::tools::execute_gh_command(tool, state)),
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![("gh_execute", visualize_gh_output as ToolVisualizer)]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::ContextTypeMeta> {
        vec![cp_base::state::ContextTypeMeta {
            context_type: "github_result",
            icon_id: "git",
            is_fixed: false,
            needs_cache: true,
            fixed_order: None,
            display_name: "github-result",
            short_name: "gh-cmd",
            needs_async_wait: false,
        }]
    }

    fn context_detail(&self, ctx: &cp_base::state::ContextElement) -> Option<String> {
        if ctx.context_type.as_str() == cp_base::state::ContextType::GITHUB_RESULT {
            Some(ctx.get_meta_str("result_command").unwrap_or("").to_string())
        } else {
            None
        }
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Git", "Git and GitHub operations")]
    }
}

/// Visualizer for gh_execute tool results.
/// Color-codes PR/issue output with status badges, labels, authors, and highlights URLs and PR numbers.
fn visualize_gh_output(content: &str, width: usize) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::prelude::*;

    let success_color = Color::Rgb(80, 250, 123); // Green for open/merged
    let error_color = Color::Rgb(255, 85, 85); // Red for closed
    let info_color = Color::Rgb(139, 233, 253); // Cyan for PR numbers
    let warning_color = Color::Rgb(241, 250, 140); // Yellow for pending/draft
    let link_color = Color::Rgb(189, 147, 249); // Purple for URLs
    let secondary_color = Color::Rgb(150, 150, 170); // Gray

    let mut lines = Vec::new();

    for line in content.lines() {
        if line.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        // Determine color based on line content
        let style = if line.starts_with("Panel created:") || line.starts_with("Panel updated:") {
            Style::default().fg(success_color)
        } else if line.starts_with("Error:") {
            Style::default().fg(error_color)
        } else if line.contains("OPEN") || line.contains("MERGED") || line.contains("✓") {
            Style::default().fg(success_color)
        } else if line.contains("CLOSED") || line.contains("✗") {
            Style::default().fg(error_color)
        } else if line.contains("DRAFT") || line.contains("PENDING") {
            Style::default().fg(warning_color)
        } else if line.contains("http://") || line.contains("https://") {
            Style::default().fg(link_color)
        } else if line.contains("#") && line.chars().any(|c| c.is_ascii_digit()) {
            // PR/issue numbers like #123
            Style::default().fg(info_color)
        } else if line.starts_with("#") {
            // Comments
            Style::default().fg(secondary_color)
        } else {
            Style::default()
        };

        let display = if line.len() > width {
            format!("{}...", &line[..line.floor_char_boundary(width.saturating_sub(3))])
        } else {
            line.to_string()
        };
        lines.push(Line::from(Span::styled(display, style)));
    }

    lines
}
