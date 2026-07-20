//! GitHub module — PR/issue management via the `gh` CLI.
//!
//! One tool: `gh_execute`. Read-only commands create auto-refreshing panels;
//! mutating commands execute directly. Depends on the git module and requires
//! `GITHUB_TOKEN` in the environment.

pub(crate) mod cache_invalidation;
/// Command classification: read-only vs mutating `gh` subcommands.
pub mod classify;
/// GitHub result panel: renders `gh` command output with caching and pagination.
mod panel;
/// Output parsing: extract PR/issue data from `gh` CLI output.
pub mod parse;
/// Tool implementations for `gh_execute`.
mod tools;
/// GitHub state types: `GithubState`, `GhCommand`, `GhWatch`.
pub mod types;

use types::GithubState;

/// Timeout for gh commands (seconds)
pub const GH_CMD_TIMEOUT_SECS: u64 = 60;

use cp_base::modules::ToolVisualizer;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ParamType, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::GithubResultPanel;
use cp_base::modules::Module;

/// Lazily parsed tool texts loaded from the GitHub YAML definition.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/github.yaml")));

/// GitHub module: PR/issue management via `gh` CLI.
#[derive(Debug, Clone, Copy)]
pub struct GithubModule;

impl Default for GithubModule {
    fn default() -> Self {
        Self::new()
    }
}

impl GithubModule {
    /// Construct the module marker (funnels cross-crate construction of this
    /// `non_exhaustive` unit struct through an associated fn).
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

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

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(Kind::GITHUB_RESULT)]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            Kind::GITHUB_RESULT => Some(Box::new(GithubResultPanel)),
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
            "gh_execute" => Some(tools::execute_gh_command(tool, state)),
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![("gh_execute", visualize_gh_output)]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![cp_base::state::context::TypeMeta {
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

    fn context_detail(&self, ctx: &cp_base::state::context::Entry) -> Option<String> {
        (ctx.context_type.as_str() == Kind::GITHUB_RESULT)
            .then(|| ctx.get_meta_str("result_command").unwrap_or("").to_owned())
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Git", "Git and GitHub operations")]
    }

    fn is_core(&self) -> bool {
        false
    }

    fn is_global(&self) -> bool {
        false
    }

    fn save_module_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_module_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn pre_flight(&self, _tool: &ToolUse, _state: &State) -> Option<Verdict> {
        None
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![]
    }

    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
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

    fn on_stream_chunk(&self, _text: &str, _state: &mut State) {}

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

/// Panel notices and error lines.
fn gh_notice_semantic(line: &str) -> Option<cp_render::Semantic> {
    use cp_render::Semantic;
    if line.starts_with("Panel created:") || line.starts_with("Panel updated:") {
        Some(Semantic::Success)
    } else if line.starts_with("Error:") {
        Some(Semantic::Error)
    } else if line.starts_with('#') {
        Some(Semantic::Muted)
    } else {
        None
    }
}

/// PR/issue state badges (open/merged/closed/draft).
fn gh_state_semantic(line: &str) -> Option<cp_render::Semantic> {
    use cp_render::Semantic;
    if line.contains("OPEN") || line.contains("MERGED") || line.contains('\u{2713}') {
        Some(Semantic::Success)
    } else if line.contains("CLOSED") || line.contains('\u{2717}') {
        Some(Semantic::Error)
    } else if line.contains("DRAFT") || line.contains("PENDING") {
        Some(Semantic::Warning)
    } else {
        None
    }
}

/// URLs and issue/PR number references.
fn gh_ref_semantic(line: &str) -> Option<cp_render::Semantic> {
    use cp_render::Semantic;
    if line.contains("http://") || line.contains("https://") {
        Some(Semantic::Accent)
    } else if line.contains('#') && line.chars().any(|c| c.is_ascii_digit()) {
        Some(Semantic::Info)
    } else {
        None
    }
}

/// Pick the semantic color for one line of `gh_execute` output.
fn gh_line_semantic(line: &str) -> cp_render::Semantic {
    gh_notice_semantic(line)
        .or_else(|| gh_state_semantic(line))
        .or_else(|| gh_ref_semantic(line))
        .unwrap_or(cp_render::Semantic::Default)
}

/// Visualizer for `gh_execute` tool results.
/// Color-codes PR/issue output with status badges, labels, authors, and highlights URLs and PR numbers.
fn visualize_gh_output(content: &str, width: usize) -> Vec<cp_render::Block> {
    use cp_render::{Block, Span};

    content
        .lines()
        .map(|line| {
            if line.is_empty() {
                return Block::empty();
            }
            let semantic = gh_line_semantic(line);
            let display = if line.len() > width {
                format!("{}...", line.get(..line.floor_char_boundary(width.saturating_sub(3))).unwrap_or(""))
            } else {
                line.to_owned()
            };
            Block::Line(vec![Span::styled(display, semantic)])
        })
        .collect()
}
