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
use std::process::Command;

/// Resolve the current branch name, or `detached:<short-sha>` for a detached HEAD.
fn current_branch() -> Option<String> {
    let output = Command::new("git").args(["branch", "--show-current"]).output().ok()?;
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !branch.is_empty() {
        return Some(branch);
    }
    // Detached HEAD: fall back to the short commit hash.
    let head = Command::new("git").args(["rev-parse", "--short", "HEAD"]).output().ok()?;
    Some(format!("detached:{}", String::from_utf8_lossy(&head.stdout).trim()))
}

/// Parse a `git diff --numstat` line into (additions, deletions, path).
/// Binary files (`-`/`-` counts) yield 0/0.
fn parse_numstat_line(line: &str) -> Option<(i32, i32, String)> {
    let parts: Vec<&str> = line.split('\t').collect();
    let (Some(add_str), Some(del_str), Some(path_str)) = (parts.first(), parts.get(1), parts.get(2)) else {
        return None;
    };
    let additions = add_str.parse::<i32>().unwrap_or(0i32);
    let deletions = del_str.parse::<i32>().unwrap_or(0i32);
    Some((additions, deletions, (*path_str).to_owned()))
}

/// Collect tracked (working-tree) changes via `git diff --numstat <base>`.
fn collect_tracked_changes(diff_args: &[&str]) -> Vec<GitFileChange> {
    let mut changes = Vec::new();
    let Ok(output) = Command::new("git").args(diff_args).output() else {
        return changes;
    };
    if !output.status.success() {
        return changes;
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some((additions, deletions, path)) = parse_numstat_line(line) {
            let change_type =
                if std::path::Path::new(&path).exists() { GitChangeType::Modified } else { GitChangeType::Deleted };
            changes.push(GitFileChange { path, additions, deletions, change_type });
        }
    }
    changes
}

/// Append staged changes (`git diff --numstat --cached`) not already present.
fn append_staged_changes(changes: &mut Vec<GitFileChange>) {
    let Ok(output) = Command::new("git").args(["diff", "--numstat", "--cached"]).output() else {
        return;
    };
    if !output.status.success() {
        return;
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some((additions, deletions, path)) = parse_numstat_line(line) {
            if changes.iter().any(|f| f.path == path) {
                continue;
            }
            changes.push(GitFileChange { path, additions, deletions, change_type: GitChangeType::Added });
        }
    }
}

/// Append untracked files (`git ls-files --others`) with their line counts.
fn append_untracked_files(changes: &mut Vec<GitFileChange>) {
    let Ok(output) = Command::new("git").args(["ls-files", "--others", "--exclude-standard"]).output() else {
        return;
    };
    if !output.status.success() {
        return;
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let path = line.trim().to_owned();
        if path.is_empty() {
            continue;
        }
        let line_count = std::fs::read_to_string(&path).map_or(0i32, |c| c.lines().count().to_i32());
        changes.push(GitFileChange {
            path,
            additions: line_count,
            deletions: 0,
            change_type: GitChangeType::Untracked,
        });
    }
}

/// Refresh git status (branch, file changes) into `GitState`.
/// Called periodically by the overview panel to keep stats up to date.
pub fn refresh_git_status(state: &mut State) {
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

    gs.branch = current_branch();

    // numstat base: an explicit diff_base, else HEAD.
    let diff_base = gs.diff_base.clone();
    let diff_args = diff_base
        .as_ref()
        .map_or_else(|| vec!["diff", "--numstat", "HEAD"], |base| vec!["diff", "--numstat", base.as_str()]);

    let mut file_changes = collect_tracked_changes(&diff_args);
    append_staged_changes(&mut file_changes);
    append_untracked_files(&mut file_changes);

    GitState::get_mut(state).file_changes = file_changes;
}

/// Timeout for git commands (seconds)
pub const GIT_CMD_TIMEOUT_SECS: u64 = 30;

/// Hard byte cap on the git section of the Overview panel (context + content).
///
/// A broken `.gitignore` (commonly during a rebase) can make `git` report
/// thousands of changed files; without a cap the per-file table grows to 1M+
/// tokens and destroys the agent's context. Beyond this budget the section is
/// truncated with a `(Rest hidden, N kB left)` marker.
const GIT_OVERVIEW_CAP_BYTES: usize = 8 * 1024;

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
#[non_exhaustive]
pub struct GitModule;

impl Default for GitModule {
    fn default() -> Self {
        Self::new()
    }
}

impl GitModule {
    /// Construct the module marker (funnels cross-crate construction of this
    /// `non_exhaustive` unit struct through an associated fn).
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

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
            GitState::get_mut(state).diff_base = Some(v.to_owned());
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
            .then(|| ctx.get_meta_str("result_command").unwrap_or("").to_owned())
    }

    fn overview_context_section(&self, state: &State) -> Option<String> {
        let gs = GitState::get(state);
        if !gs.is_repo {
            return None;
        }
        let mut output = String::new();
        if let Some(branch) = gs.branch.as_ref() {
            let _r = write!(output, "\nGit Branch: {branch}\n");
        }
        if gs.file_changes.is_empty() {
            output.push_str("Git Status: Working tree clean\n");
        } else {
            push_changes_table(&mut output, &gs.file_changes);
        }
        cap_overview_output(&mut output);
        Some(output)
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Git", "Version control operations and repository management")]
    }

    fn watch_paths(&self, _state: &State) -> Vec<cp_base::panels::WatchSpec> {
        use cp_base::panels::WatchSpec;
        vec![
            WatchSpec::File(".git/HEAD".to_owned()),
            WatchSpec::File(".git/index".to_owned()),
            WatchSpec::File(".git/MERGE_HEAD".to_owned()),
            WatchSpec::File(".git/REBASE_HEAD".to_owned()),
            WatchSpec::File(".git/CHERRY_PICK_HEAD".to_owned()),
            WatchSpec::DirRecursive(".git/refs/heads".to_owned()),
            WatchSpec::DirRecursive(".git/refs/tags".to_owned()),
            WatchSpec::DirRecursive(".git/refs/remotes".to_owned()),
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

    fn on_stream_chunk(&self, _text: &str, _state: &mut State) {}

    fn on_tool_progress(&self, _tool_name: &str, _input_so_far: &str, _state: &mut State) {}

    fn on_tool_complete(&self, _tool_name: &str, _state: &mut State) {}
}

/// Render the changed-files table (per-file +/- and a totals row) into `output`.
fn push_changes_table(output: &mut String, changes: &[GitFileChange]) {
    output.push_str("\nGit Changes:\n\n");
    output.push_str("| File | + | - | Net |\n");
    output.push_str("|------|---|---|-----|\n");
    let mut total_add: i32 = 0;
    let mut total_del: i32 = 0;
    for file in changes {
        total_add = total_add.saturating_add(file.additions);
        total_del = total_del.saturating_add(file.deletions);
        let net = file.additions.saturating_sub(file.deletions);
        let net_str = if net >= 0i32 { format!("+{net}") } else { format!("{net}") };
        let _r = writeln!(output, "| {} | +{} | -{} | {} |", file.path, file.additions, file.deletions, net_str);
    }
    let total_net = total_add.saturating_sub(total_del);
    let total_net_str = if total_net >= 0i32 { format!("+{total_net}") } else { format!("{total_net}") };
    let _r = writeln!(output, "| **Total** | **+{total_add}** | **-{total_del}** | **{total_net_str}** |");
}

/// Hard-cap the overview section at [`GIT_OVERVIEW_CAP_BYTES`].
///
/// A broken `.gitignore` (e.g. mid-rebase) can make git report thousands of
/// changed files, ballooning this section to 1M+ tokens and wrecking the
/// agent's context. Cut on the last newline within budget so the table never
/// ends mid-row (that offset is a guaranteed UTF-8 boundary), then note the
/// elided size.
fn cap_overview_output(output: &mut String) {
    if output.len() <= GIT_OVERVIEW_CAP_BYTES {
        return;
    }
    let kb_left = output.len().saturating_sub(GIT_OVERVIEW_CAP_BYTES).div_ceil(1024);
    let cut = output
        .char_indices()
        .take_while(|entry| entry.0 <= GIT_OVERVIEW_CAP_BYTES)
        .filter(|entry| entry.1 == '\n')
        .map(|entry| entry.0)
        .last()
        .unwrap_or(0);
    output.truncate(cut);
    let _r = write!(output, "\n\n_(Rest hidden, {kb_left} kB left)_\n");
}

/// Status / message lines: panel notices, errors, staged-file markers, comments.
fn git_status_semantic(line: &str) -> Option<cp_render::Semantic> {
    use cp_render::Semantic;
    if line.starts_with("Panel created:") || line.starts_with("Panel updated:") {
        Some(Semantic::Success)
    } else if line.starts_with("Error:") || line.starts_with("fatal:") || line.starts_with("error:") {
        Some(Semantic::Error)
    } else if line.starts_with("modified:") || line.starts_with("new file:") || line.starts_with("deleted:") {
        Some(Semantic::Warning)
    } else if line.starts_with('#') {
        Some(Semantic::Muted)
    } else {
        None
    }
}

/// Diff hunk body lines: `+`/`-` additions and removals.
fn git_diff_semantic(line: &str) -> Option<cp_render::Semantic> {
    use cp_render::Semantic;
    if line.starts_with("+ ") || line.starts_with("+++ ") {
        Some(Semantic::DiffAdd)
    } else if line.starts_with("- ") || line.starts_with("--- ") {
        Some(Semantic::DiffRemove)
    } else {
        None
    }
}

/// Metadata lines: hunk headers, commit/author/date, ref pointers.
fn git_meta_semantic(line: &str) -> Option<cp_render::Semantic> {
    use cp_render::Semantic;
    let is_meta = line.starts_with("@@")
        || line.starts_with("commit ")
        || line.starts_with("Author:")
        || line.starts_with("Date:")
        || line.starts_with("* ")
        || line.contains("HEAD ->")
        || line.contains("origin/");
    is_meta.then_some(Semantic::Info)
}

/// Pick the semantic color for one line of `git_execute` output.
fn git_line_semantic(line: &str) -> cp_render::Semantic {
    git_status_semantic(line)
        .or_else(|| git_diff_semantic(line))
        .or_else(|| git_meta_semantic(line))
        .unwrap_or(cp_render::Semantic::Default)
}

/// Visualizer for `git_execute` tool results.
/// Color-codes git command output with branch names, status indicators,
/// diff hunks with +/- in green/red, file names highlighted.
fn visualize_git_output(content: &str, width: usize) -> Vec<cp_render::Block> {
    use cp_render::{Block, Span};

    content
        .lines()
        .map(|line| {
            if line.is_empty() {
                return Block::empty();
            }
            let semantic = git_line_semantic(line);
            let display = if line.len() > width {
                format!("{}...", line.get(..line.floor_char_boundary(width.saturating_sub(3))).unwrap_or(""))
            } else {
                line.to_owned()
            };
            Block::Line(vec![Span::styled(display, semantic)])
        })
        .collect()
}
