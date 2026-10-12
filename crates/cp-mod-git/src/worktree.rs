//! `Worktree` tool — moves the executing thread into a git worktree.
//!
//! Worktrees live under `<project root>/.worktrees/<branch>` (slashes become
//! `+`, which branch names may not contain, so every branch gets its own
//! folder). Git's own worktree list is the source of truth for where a
//! branch's worktree is. Entering one sets the thread's
//! [`workdir`](cp_base::state::runtime::bundle::ThreadRuntime::workdir), which
//! every path- or cwd-sensitive module resolves through
//! [`State::workdir`]. The process cwd never changes, so threads running
//! concurrently can each sit in a different worktree.

use std::path::{Path, PathBuf};
use std::process::Command;

use cp_base::config::constants::{WORKTREES_DIR, project_root};
use cp_base::modules::run_with_timeout;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolDefinition, ToolResult, ToolTexts, ToolUse};

use super::GIT_CMD_TIMEOUT_SECS;

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

/// Folder name for a new worktree: `/` → `+`. Injective because
/// [`validate_branch`] never lets `+` into a name (`a/b` ≠ `a-b`).
fn folder_name(branch: &str) -> String {
    branch.replace('/', "+")
}

/// `<root>/.worktrees/<folder_name>` — where a NEW worktree for `branch` goes.
fn worktree_dir(branch: &str) -> PathBuf {
    project_root().join(WORKTREES_DIR).join(folder_name(branch))
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

/// Path git has registered for `branch`'s worktree (main one included),
/// whether or not the folder still exists on disk.
fn registered(root: &Path, branch: &str) -> Result<Option<PathBuf>, String> {
    let listing = git(root, &["worktree", "list", "--porcelain"])?;
    Ok(find_in_porcelain(&listing, branch))
}

/// Parse `git worktree list --porcelain`: blank-line separated records of
/// `worktree <path>` / `HEAD <sha>` / `branch refs/heads/<name>` lines.
fn find_in_porcelain(listing: &str, branch: &str) -> Option<PathBuf> {
    let want = format!("branch refs/heads/{branch}");
    listing
        .split("\n\n")
        .find(|rec| rec.lines().any(|l| l == want))
        .and_then(|rec| rec.lines().find_map(|l| l.strip_prefix("worktree ")))
        .map(PathBuf::from)
}

/// Same folder, tolerant of symlinked spellings; raw compare once a side is gone.
fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

/// Point the executing thread at `dir` (`None` = project root) and refresh git panels.
fn set_workdir(state: &mut State, dir: Option<PathBuf>) {
    state.thread_mut().workdir = dir;
    cp_base::panels::mark_panels_dirty(state, Kind::GIT_RESULT);
}

/// Enter `branch`'s worktree, creating it (and the branch if new) when git has none.
fn enter(state: &mut State, branch: &str) -> Result<String, String> {
    let root = project_root();
    let (dir, created) = match registered(&root, branch)? {
        Some(dir) if same_dir(&dir, &root) => {
            return Err(format!(
                "branch '{branch}' is checked out at the project root; run Worktree(action=leave) to work there"
            ));
        }
        Some(dir) if dir.is_dir() => (dir, false),
        Some(dir) => {
            return Err(format!(
                "the worktree for '{branch}' ({}) was deleted from disk; run Worktree(action=remove, branch={branch}) to clean it up, then enter again",
                dir.display()
            ));
        }
        None => (add_worktree(&root, branch)?, true),
    };
    let shown = dir.display().to_string();
    set_workdir(state, Some(dir));
    let verb = if created { "Created and entered" } else { "Entered" };
    Ok(format!(
        "{verb} worktree {shown} (branch {branch}). Relative paths, git, gh, consoles and callbacks now run there. \
         Search still indexes the project root only."
    ))
}

/// `git worktree add` a fresh folder for `branch` (new branch when it doesn't exist).
fn add_worktree(root: &Path, branch: &str) -> Result<PathBuf, String> {
    let dir = worktree_dir(branch);
    if dir.exists() {
        return Err(format!("{} exists but is not a worktree of '{branch}'; move it away first", dir.display()));
    }
    ensure_worktrees_dir(root)?;
    let path = dir.to_string_lossy().into_owned();
    let ref_name = format!("refs/heads/{branch}");
    let args: Vec<&str> = if git(root, &["rev-parse", "--verify", "--quiet", &ref_name]).is_ok() {
        vec!["worktree", "add", &path, branch]
    } else {
        vec!["worktree", "add", "-b", branch, &path]
    };
    let _out = git(root, &args).map_err(|e| friendly_add_error(branch, &e))?;
    Ok(dir)
}

/// Turn git's "already checked out" refusal into a plain explanation.
fn friendly_add_error(branch: &str, err: &str) -> String {
    let at = ["is already used by worktree at ", "is already checked out at "]
        .iter()
        .find_map(|marker| err.split_once(marker).map(|(_, rest)| rest));
    at.map_or_else(
        || err.to_owned(),
        |rest| {
            let place = rest.lines().next().unwrap_or("").trim().trim_matches('\'');
            format!("branch '{branch}' is already checked out at {place}; a branch can live in only one worktree")
        },
    )
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

/// Delete `branch`'s worktree (the branch is kept). Refuses on uncommitted
/// changes; a folder already deleted from disk is pruned from git's records.
fn remove(state: &mut State, branch: &str) -> Result<String, String> {
    let root = project_root();
    let Some(dir) = registered(&root, branch)? else {
        return Err(format!("no worktree for branch '{branch}'"));
    };
    if same_dir(&dir, &root) {
        return Err(format!("branch '{branch}' is checked out at the project root, which cannot be removed"));
    }
    let path = dir.display().to_string();
    if dir.is_dir() {
        if !git(&dir, &["status", "--porcelain"])?.is_empty() {
            return Err(format!("{path} has uncommitted changes; commit or discard them first"));
        }
        let _out = git(&root, &["worktree", "remove", &path])?;
    } else {
        let _out = git(&root, &["worktree", "prune"])?;
    }
    let mut msg = format!("Removed worktree {path} (branch {branch} kept).");
    if state.thread().workdir.as_deref().is_some_and(|w| same_dir(w, &dir)) {
        set_workdir(state, None);
        msg.push_str(" This thread was inside it and is back at the project root.");
    }
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::{find_in_porcelain, folder_name, friendly_add_error};

    /// Two worktrees as printed by `git worktree list --porcelain`.
    const LISTING: &str = "worktree /repo\nHEAD 1\nbranch refs/heads/main\n\n\
                           worktree /repo/.worktrees/a+b\nHEAD 2\nbranch refs/heads/a/b\nprunable gitdir file points to non-existent location";

    #[test]
    fn porcelain_lookup_matches_exact_branch() {
        assert_eq!(find_in_porcelain(LISTING, "a/b").as_deref(), Some(std::path::Path::new("/repo/.worktrees/a+b")));
        assert_eq!(find_in_porcelain(LISTING, "main").as_deref(), Some(std::path::Path::new("/repo")));
        assert!(find_in_porcelain(LISTING, "a").is_none());
        assert!(find_in_porcelain(LISTING, "a-b").is_none());
    }

    #[test]
    fn folder_names_never_collide() {
        assert_ne!(folder_name("a/b"), folder_name("a-b"));
        assert_eq!(folder_name("feat/x/y"), "feat+x+y");
    }

    #[test]
    fn checked_out_elsewhere_is_explained() {
        let err = "Preparing worktree (checking out 'm')\nfatal: 'm' is already used by worktree at '/repo'";
        assert_eq!(
            friendly_add_error("m", err),
            "branch 'm' is already checked out at /repo; a branch can live in only one worktree"
        );
        assert_eq!(friendly_add_error("m", "fatal: other"), "fatal: other");
    }
}
