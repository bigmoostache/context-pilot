use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use ignore::gitignore::GitignoreBuilder;

use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::storage;
use crate::types::{TreeFileDescription, TreeState};

/// Whether to show `.context-pilot/` in the tree (opt-in via env var).
pub(crate) static SHOW_CONTEXT_PILOT: LazyLock<bool> =
    LazyLock::new(|| std::env::var("SHOW_CONTEXT_PILOT_IN_TREE").is_ok_and(|v| v == "1" || v == "true"));

/// Mark tree context cache as deprecated (needs refresh)
fn invalidate_tree_cache(state: &mut State) {
    cp_base::panels::mark_panels_dirty(state, Kind::TREE);
}

/// Compute a short hash (8-char truncated FNV-1a) for a file's contents.
///
/// Used to detect stale tree descriptions via `[!]` markers.
#[must_use]
pub fn compute_file_hash(path: &Path) -> Option<String> {
    let content = fs::read(path).ok()?;
    let hex = cp_mod_utilities::hash::compute(&content);
    Some(hex.get(..8).unwrap_or(&hex).to_owned())
}

/// Outcome of toggling one folder path.
enum ToggleChange {
    /// Folder was expanded.
    Opened,
    /// Folder (and its children) was collapsed.
    Closed,
}

/// Expand a folder: record it as open.
fn open_folder(state: &mut State, normalized: &str) {
    TreeState::get_mut(state).open_folders.push(normalized.to_owned());
}

/// Collapse a folder and every descendant folder.
fn close_folder(state: &mut State, normalized: &str) {
    let ts = TreeState::get_mut(state);
    ts.open_folders.retain(|p| p != normalized);
    let prefix = format!("{normalized}/");
    ts.open_folders.retain(|p| !p.starts_with(&prefix));
}

/// Collapse `normalized`, rejecting the root and no-op'ing when already closed.
fn do_close(state: &mut State, normalized: &str) -> Result<Option<ToggleChange>, String> {
    if normalized == "." {
        return Err("Cannot close root folder".to_owned());
    }
    if !TreeState::get(state).open_folders.contains(&normalized.to_owned()) {
        return Ok(None);
    }
    close_folder(state, normalized);
    Ok(Some(ToggleChange::Closed))
}

/// Expand `normalized`, no-op'ing when already open.
fn do_open(state: &mut State, normalized: &str) -> Option<ToggleChange> {
    if TreeState::get(state).open_folders.contains(&normalized.to_owned()) {
        return None;
    }
    open_folder(state, normalized);
    Some(ToggleChange::Opened)
}

/// Apply one folder toggle. `action` is `"open"`, `"close"`, or anything else
/// (treated as a toggle: close when open, open when closed).
fn toggle_path(state: &mut State, path: &Path, normalized: &str, action: &str) -> Result<Option<ToggleChange>, String> {
    if !path.is_dir() && normalized != "." {
        return Err(format!("{normalized}: not a directory"));
    }
    let is_open = TreeState::get(state).open_folders.contains(&normalized.to_owned());
    let want_close = match action {
        "open" => false,
        "close" => true,
        _ => is_open,
    };
    if want_close { do_close(state, normalized) } else { Ok(do_open(state, normalized)) }
}

/// Append `"{label}: a, b, c"` to `result` when `items` is non-empty.
fn push_label(result: &mut Vec<String>, label: &str, items: &[String], sep: &str) {
    if !items.is_empty() {
        result.push(format!("{label}: {}", items.join(sep)));
    }
}

/// Tallies produced by a `tree_toggle_folders` invocation.
#[derive(Default)]
struct ToggleTally {
    /// Folders newly expanded.
    opened: Vec<String>,
    /// Folders collapsed.
    closed: Vec<String>,
    /// Per-path error messages.
    errors: Vec<String>,
}

/// Apply `action` to every path, returning open/close/error tallies.
fn run_toggles(state: &mut State, paths: &[&str], action: &str) -> ToggleTally {
    let mut tally = ToggleTally::default();
    for path_str in paths {
        let path = PathBuf::from(path_str);
        let normalized = crate::render::normalize_path(&path);
        match toggle_path(state, &path, &normalized, action) {
            Ok(Some(ToggleChange::Opened)) => tally.opened.push(normalized),
            Ok(Some(ToggleChange::Closed)) => tally.closed.push(normalized),
            Ok(None) => {}
            Err(e) => tally.errors.push(e),
        }
    }
    tally
}

/// Execute `tree_toggle_folders` tool - open or close folders
pub(crate) fn execute_toggle_folders(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("tree_toggle");
    let paths = tool
        .input
        .get("paths")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();

    let action = tool.input.get("action").and_then(|v| v.as_str()).unwrap_or("toggle");

    if paths.is_empty() {
        return ToolResult::new(tool.id.clone(), "Missing 'paths' parameter".to_owned(), true);
    }

    let ToggleTally { opened, closed, errors } = run_toggles(state, &paths, action);

    let mut result = Vec::new();
    push_label(&mut result, "Opened", &opened, ", ");
    push_label(&mut result, "Closed", &closed, ", ");
    push_label(&mut result, "Errors", &errors, ", ");

    // Invalidate tree cache to trigger refresh
    if !opened.is_empty() || !closed.is_empty() {
        invalidate_tree_cache(state);
    }

    let mut tool_result = ToolResult::new(
        tool.id.clone(),
        if result.is_empty() { "No changes".to_owned() } else { result.join("\n") },
        false,
    );
    // Closing folders reduces context content — safe to preserve tempo.
    // Opening folders adds new tree content — break tempo to refresh.
    if opened.is_empty() {
        tool_result.preserves_tempo = true;
    }
    tool_result
}

/// Running tallies for a `tree_describe_files` invocation.
#[derive(Default)]
struct DescribeTally {
    /// Paths whose descriptions were newly added.
    added: Vec<String>,
    /// Paths whose descriptions were updated.
    updated: Vec<String>,
    /// Paths whose descriptions were removed.
    removed: Vec<String>,
    /// Auto-closed file panels (`"{panel_id} ({path})"`).
    auto_closed: Vec<String>,
    /// Per-entry error messages.
    errors: Vec<String>,
}

/// Whether an upsert added a new description or updated an existing one.
enum UpsertKind {
    /// A new description was inserted.
    Added,
    /// An existing description was overwritten.
    Updated,
}

/// Insert or update a description in state, reporting which happened.
fn upsert_description(state: &mut State, normalized: &str, description: String, file_hash: String) -> UpsertKind {
    let ts = TreeState::get_mut(state);
    if let Some(existing) = ts.descriptions.iter_mut().find(|d| d.path == normalized) {
        existing.description = description;
        existing.file_hash = file_hash;
        UpsertKind::Updated
    } else {
        ts.descriptions.push(TreeFileDescription { path: normalized.to_owned(), description, file_hash });
        UpsertKind::Added
    }
}

/// Handle a `delete: true` request. Returns `true` when the object was a delete
/// request (handled here, caller should stop), `false` otherwise.
fn try_delete_description(
    state: &mut State,
    desc_obj: &serde_json::Value,
    normalized: &str,
    tally: &mut DescribeTally,
) -> bool {
    if !desc_obj.get("delete").and_then(serde_json::Value::as_bool).unwrap_or(false) {
        return false;
    }
    if TreeState::get(state).descriptions.iter().any(|d| d.path == normalized) {
        TreeState::get_mut(state).descriptions.retain(|d| d.path != normalized);
        tally.removed.push(normalized.to_owned());
    }
    true
}

/// Close the open FILE panel for `normalized`, if any; returns its `"id (path)"` tag.
fn auto_close_file_panel(state: &mut State, normalized: &str, cwd: Option<&PathBuf>) -> Option<String> {
    let cwd = cwd?;
    let abs_path = cwd.join(normalized).to_string_lossy().to_string();
    let pos = state
        .context
        .iter()
        .position(|c| c.context_type.as_str() == Kind::FILE && c.get_meta_str("file_path") == Some(&abs_path))?;
    let panel_id = state.context.get(pos).map(|c| c.id.clone()).unwrap_or_default();
    let _removed = state.context.remove(pos);
    Some(format!("{panel_id} ({normalized})"))
}

/// Process one description object (add / update / delete + optional panel close).
fn describe_one(state: &mut State, desc_obj: &serde_json::Value, cwd: Option<&PathBuf>, tally: &mut DescribeTally) {
    let Some(path_str) = desc_obj.get("path").and_then(|v| v.as_str()) else {
        tally.errors.push("Missing 'path' in description".to_owned());
        return;
    };
    let path = PathBuf::from(path_str);
    let normalized = crate::render::normalize_path(&path);

    if try_delete_description(state, desc_obj, &normalized, tally) {
        return;
    }

    let Some(description) = desc_obj.get("description").and_then(|v| v.as_str()) else {
        tally.errors.push(format!("{path_str}: missing 'description'"));
        return;
    };

    if !path.exists() {
        tally.errors.push(format!("{path_str}: path not found"));
        return;
    }

    let file_hash = compute_file_hash(&path).unwrap_or_default();
    match upsert_description(state, &normalized, description.to_owned(), file_hash) {
        UpsertKind::Added => tally.added.push(normalized.clone()),
        UpsertKind::Updated => tally.updated.push(normalized.clone()),
    }

    // Auto-close the file's open panel unless close_panel=false.
    let should_close = desc_obj.get("close_panel").and_then(serde_json::Value::as_bool).unwrap_or(true);
    if should_close && let Some(tag) = auto_close_file_panel(state, &normalized, cwd) {
        tally.auto_closed.push(tag);
    }
}

/// Persist added/updated descriptions and drop removed ones from the YAML store.
fn sync_descriptions_to_yaml(state: &State, added: &[String], updated: &[String], removed: &[String]) {
    for path in added.iter().chain(updated.iter()) {
        if let Some(desc) = TreeState::get(state).descriptions.iter().find(|d| d.path == *path) {
            storage::upsert_yaml_entry(&desc.path, &desc.description);
        }
    }
    for path in removed {
        storage::remove_yaml_entry(path);
    }
}

/// Execute `tree_describe_files` tool - add/update/remove file descriptions
pub(crate) fn execute_describe_files(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("tree_describe");
    let descriptions = tool.input.get("descriptions").and_then(|v| v.as_array());

    let Some(descriptions) = descriptions else {
        return ToolResult::new(tool.id.clone(), "Missing 'descriptions' parameter".to_owned(), true);
    };

    let mut tally = DescribeTally::default();

    // Resolve CWD once for absolute path matching when auto-closing panels.
    let cwd = std::env::current_dir().ok();

    for desc_obj in descriptions {
        describe_one(state, desc_obj, cwd.as_ref(), &mut tally);
    }

    let DescribeTally { added, updated, removed, auto_closed, errors } = tally;

    let mut result = Vec::new();
    push_label(&mut result, "Added", &added, ", ");
    push_label(&mut result, "Updated", &updated, ", ");
    push_label(&mut result, "Removed", &removed, ", ");
    push_label(&mut result, "Auto-closed panels", &auto_closed, ", ");
    push_label(&mut result, "Errors", &errors, "; ");

    sync_descriptions_to_yaml(state, &added, &updated, &removed);

    // Invalidate tree cache to trigger refresh
    if !added.is_empty() || !updated.is_empty() || !removed.is_empty() {
        invalidate_tree_cache(state);
    }

    let is_error = !errors.is_empty() && added.is_empty() && updated.is_empty() && removed.is_empty();
    ToolResult::new(
        tool.id.clone(),
        if result.is_empty() { "No changes".to_owned() } else { result.join("\n") },
        is_error,
    )
}

/// Execute `edit_tree_filter` tool (keep existing functionality)
pub(crate) fn execute_edit_filter(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("tree_filter");
    let Some(filter) = tool.input.get("filter").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing 'filter' parameter".to_owned(), true);
    };

    filter.clone_into(&mut TreeState::get_mut(state).filter);

    // Invalidate tree cache to trigger refresh
    invalidate_tree_cache(state);

    ToolResult::new(tool.id.clone(), format!("Updated tree filter:\n{filter}"), false)
}

/// List directory entries (files + folders) matching a prefix, respecting the gitignore filter.
///
/// Returns entries sorted: directories first, then alphabetically (case-insensitive).
/// Used by the `@` autocomplete popup.
#[must_use]
pub fn list_dir_entries(
    tree_filter: &str,
    dir_prefix: &str,
    name_prefix: &str,
) -> Vec<cp_base::state::autocomplete::Completion> {
    let root = PathBuf::from(".");

    // Build gitignore matcher from filter
    let mut builder = GitignoreBuilder::new(&root);
    for line in tree_filter.lines() {
        let line = line.trim();
        if !line.is_empty() && !line.starts_with('#') {
            let _: Option<&mut GitignoreBuilder> = builder.add_line(None, line).ok();
        }
    }
    let gitignore = builder.build().ok();

    let dir_path = if dir_prefix.is_empty() { PathBuf::from(".") } else { PathBuf::from(dir_prefix) };

    if !dir_path.is_dir() {
        return Vec::new();
    }

    let Ok(read) = fs::read_dir(&dir_path) else { return Vec::new() };
    let prefix_lower = name_prefix.to_lowercase();

    let mut entries: Vec<cp_base::state::autocomplete::Completion> = read
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let is_dir = path.is_dir();
            let name = entry.file_name().to_string_lossy().to_string();

            // .context-pilot/ is internal rigging — hide unless explicitly opted in
            if is_dir && name == ".context-pilot" && !*SHOW_CONTEXT_PILOT {
                return None;
            }

            // Apply gitignore filter
            if let Some(gi) = gitignore.as_ref()
                && gi.matched(&path, is_dir).is_ignore()
            {
                return None;
            }

            // Prefix match (case-insensitive)
            if !prefix_lower.is_empty() && !name.to_lowercase().starts_with(&prefix_lower) {
                return None;
            }

            Some(cp_base::state::autocomplete::Completion::new(name, is_dir))
        })
        .collect();

    // Sort: directories first, then alphabetically (case-insensitive)
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    entries
}
