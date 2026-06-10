//! Workspace registry — the project layer above the (mono-)session.
//!
//! A project is a directory under the projects root (e.g.
//! `~/nestor/projects/<name>`), carrying its own `.context-pilot/` state.
//! The web server manages the registry over REST (list/create/clone/
//! archive/delete); *switching* is the core's business (pointer file +
//! exec restart), reached through [`WebEvent::SwitchProject`].

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Name of the pointer file holding the current project name.
pub const CURRENT_POINTER: &str = ".current";
/// Directory (under the root) receiving archived projects.
pub const ARCHIVE_DIR: &str = ".archive";

/// One project entry in `GET /api/projects`.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectInfo {
    /// Directory name (also the project ID).
    pub name: String,
    /// Whether this is the project the agent currently runs in.
    pub current: bool,
    /// Last activity (mtime of the project's state file, ms since epoch).
    pub last_active_ms: u64,
    /// Whether a `.git` directory is present.
    pub has_git: bool,
}

/// Body of `POST /api/projects`.
#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    /// Project name (`[A-Za-z0-9_-]{1,64}`).
    pub name: String,
    /// Optional repository to clone into the new workspace.
    #[serde(default)]
    pub git_url: Option<String>,
}

/// Body of `POST /api/projects/switch` and `/archive`.
#[derive(Debug, Deserialize)]
pub struct NameRequest {
    /// Target project name.
    pub name: String,
}

/// Body of `POST /api/projects/delete` — `confirm` must repeat the name.
#[derive(Debug, Deserialize)]
pub struct DeleteRequest {
    /// Project to delete.
    pub name: String,
    /// Must equal `name` (typed confirmation).
    pub confirm: String,
}

/// Operation errors, mapped to HTTP statuses by the server layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectError {
    /// Name fails validation.
    BadName,
    /// Project not found.
    NotFound,
    /// Project already exists.
    Exists,
    /// Operation refused on the currently active project.
    IsCurrent,
    /// Typed confirmation mismatch.
    BadConfirm,
    /// Underlying I/O or git failure (message for the client).
    Io(String),
}

/// Validate a project name: 1–64 chars of `[A-Za-z0-9_-]` (no traversal).
#[must_use]
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Current time in ms since the UNIX epoch.
fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// mtime of a path in ms since the UNIX epoch (0 when unavailable).
fn mtime_ms(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0))
}

/// Read the current project name from the pointer file.
#[must_use]
pub fn read_current(root: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(root.join(CURRENT_POINTER)).ok()?;
    let name = raw.trim().to_string();
    valid_name(&name).then_some(name)
}

/// Atomically write the current-project pointer.
///
/// # Errors
///
/// [`ProjectError::Io`] when the file cannot be written.
pub fn write_current(root: &Path, name: &str) -> Result<(), ProjectError> {
    let staging = root.join(".current.new");
    std::fs::write(&staging, name).map_err(|e| ProjectError::Io(e.to_string()))?;
    std::fs::rename(&staging, root.join(CURRENT_POINTER)).map_err(|e| ProjectError::Io(e.to_string()))
}

/// List projects (directories under the root, archive and dotfiles excluded),
/// most recently active first.
#[must_use]
pub fn list(root: &Path) -> Vec<ProjectInfo> {
    let current = read_current(root);
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut projects: Vec<ProjectInfo> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || !valid_name(&name) {
                return None;
            }
            let path = entry.path();
            // L'activité réelle vit dans l'état de session ; repli : mtime du dossier.
            let state_file = path.join(".context-pilot").join("config.json");
            let last_active_ms = if state_file.exists() { mtime_ms(&state_file) } else { mtime_ms(&path) };
            Some(ProjectInfo {
                current: current.as_deref() == Some(name.as_str()),
                name,
                last_active_ms,
                has_git: path.join(".git").is_dir(),
            })
        })
        .collect();
    projects.sort_by_key(|p| std::cmp::Reverse(p.last_active_ms));
    projects
}

/// Create a project directory; with `git_url`, clone into it.
///
/// Cloning runs `git clone <url> <dir>` and cleans the directory up on
/// failure. Blocking — call from a blocking-friendly context.
///
/// # Errors
///
/// [`ProjectError::BadName`], [`ProjectError::Exists`], [`ProjectError::Io`].
pub fn create(root: &Path, name: &str, git_url: Option<&str>) -> Result<(), ProjectError> {
    if !valid_name(name) {
        return Err(ProjectError::BadName);
    }
    let path = root.join(name);
    if path.exists() {
        return Err(ProjectError::Exists);
    }
    std::fs::create_dir_all(&path).map_err(|e| ProjectError::Io(e.to_string()))?;

    if let Some(url) = git_url.filter(|u| !u.trim().is_empty()) {
        let output = std::process::Command::new("git")
            .arg("clone")
            .arg(url.trim())
            .arg(&path)
            .output()
            .map_err(|e| ProjectError::Io(format!("git introuvable : {e}")))?;
        if !output.status.success() {
            // Nettoie le dossier pour ne pas laisser un projet à moitié né.
            let _r = std::fs::remove_dir_all(&path);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let tail: String = stderr.lines().rev().take(3).collect::<Vec<_>>().join(" / ");
            return Err(ProjectError::Io(format!("git clone a échoué : {tail}")));
        }
    }
    Ok(())
}

/// Archive a project: move it under `.archive/<name>-<timestamp>`.
///
/// # Errors
///
/// [`ProjectError::BadName`], [`ProjectError::NotFound`],
/// [`ProjectError::IsCurrent`], [`ProjectError::Io`].
pub fn archive(root: &Path, name: &str) -> Result<(), ProjectError> {
    let path = checked_path(root, name)?;
    let archive_root = root.join(ARCHIVE_DIR);
    std::fs::create_dir_all(&archive_root).map_err(|e| ProjectError::Io(e.to_string()))?;
    let dest = archive_root.join(format!("{name}-{}", now_ms()));
    std::fs::rename(&path, &dest).map_err(|e| ProjectError::Io(e.to_string()))
}

/// Permanently delete a project (the confirmation must repeat its name).
///
/// # Errors
///
/// [`ProjectError::BadConfirm`] plus everything [`archive`] can return.
pub fn delete(root: &Path, name: &str, confirm: &str) -> Result<(), ProjectError> {
    if name != confirm {
        return Err(ProjectError::BadConfirm);
    }
    let path = checked_path(root, name)?;
    std::fs::remove_dir_all(&path).map_err(|e| ProjectError::Io(e.to_string()))
}

/// Resolve and validate a project path for a destructive operation:
/// valid name, existing directory, and not the current project.
fn checked_path(root: &Path, name: &str) -> Result<PathBuf, ProjectError> {
    if !valid_name(name) {
        return Err(ProjectError::BadName);
    }
    if read_current(root).as_deref() == Some(name) {
        return Err(ProjectError::IsCurrent);
    }
    let path = root.join(name);
    if !path.is_dir() {
        return Err(ProjectError::NotFound);
    }
    Ok(path)
}
