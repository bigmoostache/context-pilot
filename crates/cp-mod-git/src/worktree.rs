//! `Worktree` tool — moves the executing thread into a git worktree.
//!
//! Worktrees live under `<project root>/.worktrees/<branch>` (slashes become
//! dashes). Entering one sets the thread's
//! [`workdir`](cp_base::state::runtime::bundle::ThreadRuntime::workdir), which
//! every path- or cwd-sensitive module resolves through
//! [`State::workdir`]. The process cwd never changes, so threads running
//! concurrently can each sit in a different worktree.

use std::path::{Path, PathBuf};
use std::process::Command;

use cp_base::config::constants::project_root;
use cp_base::modules::run_with_timeout;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolDefinition, ToolResult, ToolTexts, ToolUse};

use super::GIT_CMD_TIMEOUT_SECS;

/// Folder (under the project root) holding every managed worktree.
const WORKTREES_DIR: &str = ".worktrees";

/// Tool definition, built here to keep `lib.rs` under the line cap.
pub(crate) fn definition(t: &ToolTexts) -> ToolDefinition {
    ToolDefinition::from_yaml("Worktree", t)
        .declares_task()
        .short_desc("Move this thread into a git worktree")
        .category("Git")
        .param_enum("action", &["enter", "leave", "list", "remove"], true)
        .param("branch", cp_base::tools::ParamType::String, false)
        .build()
}

/// Dispatch one `Worktree` call.
pub(crate) fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let action = tool.input.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let branch = tool.input.get("branch").and_then(|v| v.as_str());
    let outcome = match (action, branch) {
        ("leave", _) => Ok(leave(state)),
        ("list", _) => list(state),
        ("enter", Some(b)) => validate_branch(b).and_then(|()| enter(state, b)),
        ("remove", Some(b)) => validate_branch(b).and_then(|()| remove(state, b)),
        ("enter" | "remove", None) => Err(format!("'branch' is required for action '{action}'")),
        _ => Err(format!("Unknown action '{action}' (expected enter|leave|list|remove)")),
    };
    match outcome {
        Ok(msg) => ToolResult::new(tool.id.clone(), msg, false),
        Err(msg) => ToolResult::new(tool.id.clone(), format!("Error: {msg}"), true),
    }
}

/// Reject names git would read as options or that escape `.worktrees/`.
fn validate_branch(branch: &str) -> Result<(), String> {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-');
    if branch.is_empty() || branch.starts_with('-') || branch.contains("..") || !branch.chars().all(allowed) {
        return Err(format!("invalid branch name '{branch}'"));
    }
    Ok(())
}

/// `<root>/.worktrees/<branch>` with `/` flattened to `-`.
fn worktree_dir(branch: &str) -> PathBuf {
    project_root().join(WORKTREES_DIR).join(branch.replace('/', "-"))
}

/// Run `git <args>` in `dir`; stdout on success, stderr (or stdout) on failure.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    let _c = cmd
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_TERMINAL_PROMPT", "0");
    let out = run_with_timeout(cmd, GIT_CMD_TIMEOUT_SECS).map_err(|e| format!("git failed to run: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if out.status.success() {
        return Ok(stdout);
    }
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    Err(if stderr.is_empty() { stdout } else { stderr })
}

/// Point the executing thread at `dir` (`None` = project root) and refresh git panels.
fn set_workdir(state: &mut State, dir: Option<PathBuf>) {
    state.thread_mut().workdir = dir;
    cp_base::panels::mark_panels_dirty(state, Kind::GIT_RESULT);
}

/// Create the worktree if missing (new branch when it doesn't exist), then enter it.
fn enter(state: &mut State, branch: &str) -> Result<String, String> {
    let root = project_root();
    let dir = worktree_dir(branch);
    let created = !dir.is_dir();
    if created {
        ensure_worktrees_dir(&root)?;
        let path = dir.to_string_lossy().into_owned();
        let ref_name = format!("refs/heads/{branch}");
        let args: Vec<&str> = if git(&root, &["rev-parse", "--verify", "--quiet", &ref_name]).is_ok() {
            vec!["worktree", "add", &path, branch]
        } else {
            vec!["worktree", "add", "-b", branch, &path]
        };
        let _out = git(&root, &args)?;
    } else {
        // `a/b` and `a-b` share a folder: never land on the wrong branch.
        let head = git(&dir, &["branch", "--show-current"])?;
        if head != branch {
            return Err(format!("{} is checked out on '{head}', not '{branch}'", dir.display()));
        }
    }
    let shown = dir.display().to_string();
    set_workdir(state, Some(dir));
    let verb = if created { "Created and entered" } else { "Entered" };
    Ok(format!(
        "{verb} worktree {shown} (branch {branch}). Relative paths, git, gh, consoles and callbacks now run there. \
         Search still indexes the project root only."
    ))
}

/// Create `.worktrees/` with a self-ignoring `.gitignore`, leaving the repo's own untouched.
fn ensure_worktrees_dir(root: &Path) -> Result<(), String> {
    let base = root.join(WORKTREES_DIR);
    std::fs::create_dir_all(&base).map_err(|e| format!("cannot create {}: {e}", base.display()))?;
    let ignore = base.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(&ignore, "*\n").map_err(|e| format!("cannot write {}: {e}", ignore.display()))?;
    }
    Ok(())
}

/// Return the executing thread to the project root.
fn leave(state: &mut State) -> String {
    let was = state.thread().workdir.as_ref().map(|d| d.display().to_string());
    set_workdir(state, None);
    was.map_or_else(
        || "Already at the project root.".to_owned(),
        |d| format!("Left worktree {d}; back at the project root."),
    )
}

/// `git worktree list` plus where this thread currently works.
fn list(state: &State) -> Result<String, String> {
    let listing = git(&project_root(), &["worktree", "list"])?;
    Ok(format!("{listing}\n\nThis thread works in: {}", state.workdir().display()))
}

/// Delete a clean worktree (the branch is kept); refuses on uncommitted changes.
fn remove(state: &mut State, branch: &str) -> Result<String, String> {
    let dir = worktree_dir(branch);
    if !dir.is_dir() {
        return Err(format!("no worktree at {}", dir.display()));
    }
    if !git(&dir, &["status", "--porcelain"])?.is_empty() {
        return Err(format!("{} has uncommitted changes; commit or discard them first", dir.display()));
    }
    let path = dir.to_string_lossy().into_owned();
    let _out = git(&project_root(), &["worktree", "remove", &path])?;
    if state.thread().workdir.as_deref() == Some(dir.as_path()) {
        set_workdir(state, None);
    }
    Ok(format!("Removed worktree {path} (branch {branch} kept)."))
}
