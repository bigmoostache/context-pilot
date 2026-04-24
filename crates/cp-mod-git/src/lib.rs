//! Git module — version control integration via the `git` CLI.
//!
//! One tool: `git_execute`. Read-only commands (log, diff, status, etc.) create
//! auto-refreshing dynamic panels. Mutating commands (commit, push, merge, etc.)
//! execute directly and return output. Shell operators are blocked for safety.

/// Cache invalidation rules for git result panels.
pub(crate) mod cache_invalidation;
/// Git command classification (read-only vs mutating).
mod classify;
/// Panel implementation for displaying git command results.
mod result_panel;
/// Tool execution logic for `git_execute`.
mod tools;
/// Git state types: `GitState`, `GitFileChange`, `GitChangeType`.
pub mod types;

use types::{GitChangeType, GitFileChange, GitState};

use cp_base::cast::Safe as _;
use std::fmt::Write as _;

/// Refresh git status (branch, file changes) into `GitState`.
/// Called periodically by the overview panel to keep stats up to date.
pub fn refresh_git_status(state: &mut State) {
    use std::process::Command;

    // Check if git repo
    let is_repo = Command::new("git").args(["rev-parse", "--git-dir"]).output().is_ok_and(|o| o.status.success());

    let gs = GitState::get_mut(state);
    gs.is_repo = is_repo;

    if !is_repo {
        gs.branch = None;
        gs.branches = vec![];
        gs.file_changes = vec![];
        return;
    }

    // Get current branch
    if let Ok(output) = Command::new("git").args(["branch", "--show-current"]).output() {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if branch.is_empty() {
            // Detached HEAD
            if let Ok(o2) = Command::new("git").args(["rev-parse", "--short", "HEAD"]).output() {
                gs.branch = Some(format!("detached:{}", String::from_utf8_lossy(&o2.stdout).trim()));
            }
        } else {
            gs.branch = Some(branch);
        }
    }

    // Get file changes with numstat
    let diff_base = gs.diff_base.clone();
    let diff_args = diff_base
        .as_ref()
        .map_or_else(|| vec!["diff", "--numstat", "HEAD"], |base| vec!["diff", "--numstat", base.as_str()]);

    let mut file_changes: Vec<GitFileChange> = Vec::new();

    // Tracked changes (diff against HEAD or base)
    if let Ok(output) = Command::new("git").args(&diff_args).output()
        && output.status.success()
    {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            let [add_str, del_str, path_str, ..] = parts.as_slice() else {
                continue;
            };
            let additions = add_str.parse::<i32>().unwrap_or(0);
            let deletions = del_str.parse::<i32>().unwrap_or(0);
            let path = (*path_str).to_string();

            // Check if file exists to determine if deleted
            let change_type =
                if std::path::Path::new(&path).exists() { GitChangeType::Modified } else { GitChangeType::Deleted };

            file_changes.push(GitFileChange { path, additions, deletions, change_type });
        }
    }

    // Staged changes (diff --cached)
    if let Ok(output) = Command::new("git").args(["diff", "--numstat", "--cached"]).output()
        && output.status.success()
    {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            let [add_str, del_str, path_str, ..] = parts.as_slice() else {
                continue;
            };
            let additions = add_str.parse::<i32>().unwrap_or(0);
            let deletions = del_str.parse::<i32>().unwrap_or(0);
            let path = (*path_str).to_string();

            // Skip if already in the list
            if file_changes.iter().any(|f| f.path == path) {
                continue;
            }

            file_changes.push(GitFileChange { path, additions, deletions, change_type: GitChangeType::Added });
        }
    }

    // Untracked files
    if let Ok(output) = Command::new("git").args(["ls-files", "--others", "--exclude-standard"]).output()
        && output.status.success()
    {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let path = line.trim().to_string();
            if path.is_empty() {
                continue;
            }
            // Count lines for untracked files
            let line_count = std::fs::read_to_string(&path).map_or(0, |c| c.lines().count().to_i32());

            file_changes.push(GitFileChange {
                path,
                additions: line_count,
                deletions: 0,
                change_type: GitChangeType::Untracked,
            });
        }
    }

    let gs_mut = GitState::get_mut(state);
    gs_mut.file_changes = file_changes;
}

/// Timeout for git commands (seconds)
pub const GIT_CMD_TIMEOUT_SECS: u64 = 30;

/// Refresh interval for git status (milliseconds)
pub(crate) const GIT_STATUS_REFRESH_MS: u64 = 2_000; // 2 seconds

use serde_json::json;

use cp_base::modules::ToolVisualizer;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ParamType, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::result_panel::GitResultPanel;
use cp_base::modules::Module;

/// Parsed tool description YAML for the git module.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/git.yaml")));

/// Git module: version control tools, status tracking, and result panels.
#[derive(Debug, Clone, Copy)]
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

    fn init_state(&self, state: &mut State) {
        state.set_ext(GitState::new());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(GitState::new());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let gs = GitState::get(state);
        json!({
            "git_diff_base": gs.diff_base,
        })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(v) = data.get("git_diff_base").and_then(|v| v.as_str()) {
            GitState::get_mut(state).diff_base = Some(v.to_string());
        }
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(Kind::GIT_RESULT)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            Kind::GIT_RESULT => Some(Box::new(GitResultPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("git_execute", t)
                .short_desc("Run git commands")
                .category("Git")
                .param("command", ParamType::String, true)
                .build(),
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "git_execute" => Some(tools::execute_git_command(tool, state)),
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![("git_execute", visualize_git_output)]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![cp_base::state::context::TypeMeta {
            context_type: "git_result",
            icon_id: "git",
            is_fixed: false,
            needs_cache: true,
            fixed_order: None,
            display_name: "git-result",
            short_name: "git-cmd",
            needs_async_wait: false,
        }]
    }

    fn context_detail(&self, ctx: &cp_base::state::context::Entry) -> Option<String> {
        (ctx.context_type.as_str() == Kind::GIT_RESULT)
            .then(|| ctx.get_meta_str("result_command").unwrap_or("").to_string())
    }

    fn overview_context_section(&self, state: &State) -> Option<String> {
        let gs = GitState::get(state);
        if !gs.is_repo {
            return None;
        }
        let mut output = String::new();
        if let Some(branch) = &gs.branch {
            let _r = write!(output, "\nGit Branch: {branch}\n");
        }
        if gs.file_changes.is_empty() {
            output.push_str("Git Status: Working tree clean\n");
        } else {
            output.push_str("\nGit Changes:\n\n");
            output.push_str("| File | + | - | Net |\n");
            output.push_str("|------|---|---|-----|\n");
            let mut total_add: i32 = 0;
            let mut total_del: i32 = 0;
            for file in &gs.file_changes {
                total_add = total_add.saturating_add(file.additions);
                total_del = total_del.saturating_add(file.deletions);
                let net = file.additions.saturating_sub(file.deletions);
                let net_str = if net >= 0 { format!("+{net}") } else { format!("{net}") };
                let _r =
                    writeln!(output, "| {} | +{} | -{} | {} |", file.path, file.additions, file.deletions, net_str);
            }
            let total_net = total_add.saturating_sub(total_del);
            let total_net_str = if total_net >= 0 { format!("+{total_net}") } else { format!("{total_net}") };
            let _r = writeln!(output, "| **Total** | **+{total_add}** | **-{total_del}** | **{total_net_str}** |");
        }
        Some(output)
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Git", "Version control operations and repository management")]
    }

    fn watch_paths(&self, _state: &State) -> Vec<cp_base::panels::WatchSpec> {
        use cp_base::panels::WatchSpec;
        vec![
            WatchSpec::File(".git/HEAD".to_string()),
            WatchSpec::File(".git/index".to_string()),
            WatchSpec::File(".git/MERGE_HEAD".to_string()),
            WatchSpec::File(".git/REBASE_HEAD".to_string()),
            WatchSpec::File(".git/CHERRY_PICK_HEAD".to_string()),
            WatchSpec::DirRecursive(".git/refs/heads".to_string()),
            WatchSpec::DirRecursive(".git/refs/tags".to_string()),
            WatchSpec::DirRecursive(".git/refs/remotes".to_string()),
        ]
    }

    fn should_invalidate_on_fs_change(
        &self,
        ctx: &cp_base::state::context::Entry,
        changed_path: &str,
        _is_dir_event: bool,
    ) -> bool {
        ctx.context_type.as_str() == Kind::GIT_RESULT && changed_path.starts_with(".git/")
    }

    fn watcher_immediate_refresh(&self) -> bool {
        false // Prevent feedback loop: git status writes .git/index
    }

    fn dependencies(&self) -> &[&'static str] {
        &[]
    }
    fn is_core(&self) -> bool {
        false
    }
    fn is_global(&self) -> bool {
        false
    }
    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }
    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}
    fn pre_flight(&self, _tool: &ToolUse, _state: &State) -> Option<cp_base::tools::pre_flight::Verdict> {
        None
    }
    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
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

    fn on_tool_progress(&self, _tool_name: &str, _input_so_far: &str, _state: &mut State) {}

    fn on_tool_complete(&self, _tool_name: &str, _state: &mut State) {}
}

/// Visualizer for `git_execute` tool results.
/// Color-codes git command output with branch names, status indicators,
/// diff hunks with +/- in green/red, file names highlighted.
fn visualize_git_output(content: &str, width: usize) -> Vec<cp_render::Block> {
    use cp_render::{Block, Semantic, Span};

    content
        .lines()
        .map(|line| {
            if line.is_empty() {
                return Block::empty();
            }
            let semantic = if line.starts_with("Panel created:") || line.starts_with("Panel updated:") {
                Semantic::Success
            } else if line.starts_with("Error:") || line.starts_with("fatal:") || line.starts_with("error:") {
                Semantic::Error
            } else if line.starts_with("+ ") || line.starts_with("+++ ") {
                Semantic::DiffAdd
            } else if line.starts_with("- ") || line.starts_with("--- ") {
                Semantic::DiffRemove
            } else if line.starts_with("@@")
                || line.starts_with("commit ")
                || line.starts_with("Author:")
                || line.starts_with("Date:")
                || line.starts_with("* ")
                || line.contains("HEAD ->")
                || line.contains("origin/")
            {
                Semantic::Info
            } else if line.starts_with("modified:") || line.starts_with("new file:") || line.starts_with("deleted:") {
                Semantic::Warning
            } else if line.starts_with('#') {
                Semantic::Muted
            } else {
                Semantic::Default
            };
            let display = if line.len() > width {
                format!("{}...", line.get(..line.floor_char_boundary(width.saturating_sub(3))).unwrap_or(""))
            } else {
                line.to_string()
            };
            Block::Line(vec![Span::styled(display, semantic)])
        })
        .collect()
}
