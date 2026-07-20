//! Callback trigger engine: collect changed files, match patterns, partition callbacks.
//!
//! Called from tool_pipeline.rs after a batch of Edit/Write tools completes.
//! Firing logic lives in firing.rs.

use std::path::Path;

use globset::Glob;

use cp_base::state::runtime::State;

use crate::types::{CallbackDefinition, CallbackState};

/// A callback that matched one or more changed files and is ready to fire.
// Queue ID test marker — delete me later
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MatchedCallback {
    /// The callback definition
    pub definition: CallbackDefinition,
    /// Files that matched this callback's pattern (relative paths)
    pub matched_files: Vec<String>,
}

/// A changed file with optional `skip_callbacks` names from the tool that changed it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ChangedFile {
    /// Relative path to the changed file
    pub path: String,
    /// Callback names the LLM wants to skip for this file
    pub skip_callbacks: Vec<String>,
}

/// Normalize a raw tool `file_path` to a project-relative path (strip `./` and
/// an absolute project-root prefix).
fn normalize_changed_path(path: &str, project_root: &str) -> String {
    let mut anchor = path.strip_prefix("./").unwrap_or(path);
    if let Some(relative) = anchor.strip_prefix(project_root) {
        anchor = relative.strip_prefix('/').unwrap_or(relative);
    }
    anchor.to_owned()
}

/// Merge one changed file into `hull`, unioning skip-lists when the path
/// already appears (two tools touched the same file in one batch).
fn merge_changed_file(hull: &mut Vec<ChangedFile>, path: String, skip_names: Vec<String>) {
    if let Some(existing) = hull.iter_mut().find(|f| f.path == path) {
        for name in skip_names {
            if !existing.skip_callbacks.contains(&name) {
                existing.skip_callbacks.push(name);
            }
        }
    } else {
        hull.push(ChangedFile { path, skip_callbacks: skip_names });
    }
}

/// Collect changed file paths from a batch of tool uses.
/// Extracts `file_path` from Edit and Write tool inputs.
/// Also collects `skip_callbacks` names per tool for selective skipping.
#[must_use]
pub fn collect_changed_files(tools: &[cp_base::tools::ToolUse]) -> Vec<ChangedFile> {
    let mut hull: Vec<ChangedFile> = Vec::new();
    let project_root = std::env::current_dir().unwrap_or_default().to_string_lossy().to_string();
    for tool in tools {
        if !matches!(tool.name.as_str(), "Edit" | "Write") {
            continue;
        }
        let Some(path) = tool.input.get("file_path").and_then(|v| v.as_str()) else {
            continue;
        };
        let anchor = normalize_changed_path(path, &project_root);
        let skip_names = parse_skip_callbacks(&tool.input);
        merge_changed_file(&mut hull, anchor, skip_names);
    }
    hull
}

/// Parse the `skip_callbacks` parameter from a tool's input.
/// Accepts a JSON array of strings (callback names).
fn parse_skip_callbacks(input: &serde_json::Value) -> Vec<String> {
    input
        .get("skip_callbacks")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// True if `def`'s glob matches `file` (full path or basename).
fn def_matches_file(matcher: &globset::GlobMatcher, file: &str) -> bool {
    let path = Path::new(file);
    matcher.is_match(path) || matcher.is_match(path.file_name().unwrap_or_default())
}

/// Match one callback definition against all changed files, honoring
/// `skip_callbacks`. Returns the matched file paths and pushes a warning for
/// any skip that named this callback but wouldn't have triggered it anyway.
fn match_one_def(def: &CallbackDefinition, changed_files: &[ChangedFile], warnings: &mut Vec<String>) -> Vec<String> {
    let Ok(glob) = Glob::new(&def.pattern) else {
        return Vec::new();
    };
    let matcher = glob.compile_matcher();
    let mut crew = Vec::new();

    for changed_file in changed_files {
        let skipped = changed_file.skip_callbacks.iter().any(|name| name == &def.name);
        let would_match = def_matches_file(&matcher, &changed_file.path);
        if skipped {
            if !would_match {
                warnings.push(format!(
                    "skip_callbacks: '{}' would not have triggered for '{}' (pattern '{}' doesn't match)",
                    def.name, changed_file.path, def.pattern,
                ));
            }
            continue;
        }
        if would_match {
            crew.push(changed_file.path.clone());
        }
    }
    crew
}

/// Match changed files against active callback patterns.
///
/// Returns a list of callbacks that matched, each with their matched files.
/// Also validates `skip_callbacks` names and returns warnings for non-existent or non-matching ones.
#[must_use]
pub fn match_callbacks(state: &State, changed_files: &[ChangedFile]) -> (Vec<MatchedCallback>, Vec<String>) {
    let _fg = cp_base::flame!("cb_match");
    if changed_files.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let cs = CallbackState::get(state);
    let mut treasure_map: Vec<MatchedCallback> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Validate skip_callbacks names across all files
    let all_skip_names: Vec<&str> =
        changed_files.iter().flat_map(|f| f.skip_callbacks.iter().map(String::as_str)).collect();
    validate_skip_names(cs, &all_skip_names, &mut warnings);

    for def in &cs.definitions {
        let crew = match_one_def(def, changed_files, &mut warnings);
        if !crew.is_empty() {
            treasure_map.push(MatchedCallback { definition: def.clone(), matched_files: crew });
        }
    }

    (treasure_map, warnings)
}

/// Validate `skip_callbacks` names against known callback definitions.
/// Warns on names that don't match any defined callback.
fn validate_skip_names(cs: &CallbackState, names: &[&str], warnings: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    for name in names {
        if seen.contains(name) {
            continue;
        }
        let _inserted = seen.insert(*name);
        if !cs.definitions.iter().any(|d| d.name == *name) {
            warnings.push(format!("skip_callbacks: '{name}' does not match any defined callback"));
        }
    }
}

/// Separate matched callbacks into blocking and non-blocking groups.
#[must_use]
pub fn partition_callbacks(matched: Vec<MatchedCallback>) -> (Vec<MatchedCallback>, Vec<MatchedCallback>) {
    let mut blocking_fleet = Vec::new();
    let mut async_fleet = Vec::new();

    for cb in matched {
        if cb.definition.blocking {
            blocking_fleet.push(cb);
        } else {
            async_fleet.push(cb);
        }
    }

    (blocking_fleet, async_fleet)
}

/// Build the $`CP_CHANGED_FILES` environment variable value (newline-separated).
#[must_use]
pub fn build_changed_files_env(files: &[String]) -> String {
    files.join("\n")
}
