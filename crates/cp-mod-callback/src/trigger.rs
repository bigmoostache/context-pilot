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
pub struct MatchedCallback {
    /// The callback definition
    pub definition: CallbackDefinition,
    /// Files that matched this callback's pattern (relative paths)
    pub matched_files: Vec<String>,
}

/// A changed file with optional `skip_callbacks` names from the tool that changed it.
#[derive(Debug, Clone)]
pub struct ChangedFile {
    /// Relative path to the changed file
    pub path: String,
    /// Callback names the LLM wants to skip for this file
    pub skip_callbacks: Vec<String>,
}

/// Collect changed file paths from a batch of tool uses.
/// Extracts `file_path` from Edit and Write tool inputs.
/// Also collects `skip_callbacks` names per tool for selective skipping.
#[must_use]
pub fn collect_changed_files(tools: &[cp_base::tools::ToolUse]) -> Vec<ChangedFile> {
    let mut hull: Vec<ChangedFile> = Vec::new();
    let project_root = std::env::current_dir().unwrap_or_default().to_string_lossy().to_string();
    for tool in tools {
        match tool.name.as_str() {
            "Edit" | "Write" => {
                if let Some(path) = tool.input.get("file_path").and_then(|v| v.as_str()) {
                    // Normalize: strip leading ./ if present, strip absolute project root prefix
                    let mut anchor_path = path.strip_prefix("./").unwrap_or(path);
                    if let Some(relative) = anchor_path.strip_prefix(&project_root) {
                        anchor_path = relative.strip_prefix('/').unwrap_or(relative);
                    }
                    let anchor_str = anchor_path.to_string();

                    // Parse skip_callbacks: string array of callback names
                    let skip_names = parse_skip_callbacks(&tool.input);

                    // Merge: if file already in hull, union the skip lists
                    if let Some(existing) = hull.iter_mut().find(|f| f.path == anchor_str) {
                        for name in &skip_names {
                            if !existing.skip_callbacks.contains(name) {
                                existing.skip_callbacks.push(name.clone());
                            }
                        }
                    } else {
                        hull.push(ChangedFile { path: anchor_str, skip_callbacks: skip_names });
                    }
                }
            }
            _ => {}
        }
    }
    hull
}

/// Parse the `skip_callbacks` parameter from a tool's input.
/// Accepts a JSON array of strings (callback names).
fn parse_skip_callbacks(input: &serde_json::Value) -> Vec<String> {
    input
        .get("skip_callbacks")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|item| item.as_str().map(ToString::to_string)).collect())
        .unwrap_or_default()
}

/// Match changed files against active callback patterns.
///
/// Returns a list of callbacks that matched, each with their matched files.
/// Also validates `skip_callbacks` names and returns warnings for non-existent or non-matching ones.
#[must_use]
pub fn match_callbacks(state: &State, changed_files: &[ChangedFile]) -> (Vec<MatchedCallback>, Vec<String>) {
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
        // Only fire active callbacks
        if !cs.active_set.contains(&def.id) {
            continue;
        }

        // Compile the glob pattern
        let compass = match Glob::new(&def.pattern) {
            Ok(g) => g.compile_matcher(),
            Err(_) => continue,
        };

        // Match each changed file against the pattern, respecting skip_callbacks
        let mut crew: Vec<String> = Vec::new();
        for changed_file in changed_files {
            // Check if this callback is skipped for this file
            if changed_file.skip_callbacks.iter().any(|name| name == &def.name) {
                continue;
            }

            let path = Path::new(&changed_file.path);
            if compass.is_match(path) || compass.is_match(path.file_name().unwrap_or_default()) {
                crew.push(changed_file.path.clone());
            }
        }

        // Warn if a skip_callbacks name matched this callback but the file wouldn't have triggered it
        for changed_file in changed_files {
            if changed_file.skip_callbacks.iter().any(|name| name == &def.name) {
                let path = Path::new(&changed_file.path);
                let would_match = compass.is_match(path) || compass.is_match(path.file_name().unwrap_or_default());
                if !would_match {
                    warnings.push(format!(
                        "skip_callbacks: '{}' would not have triggered for '{}' (pattern '{}' doesn't match)",
                        def.name, changed_file.path, def.pattern,
                    ));
                }
            }
        }

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
        let _ = seen.insert(*name);
        if !cs.definitions.iter().any(|d| d.name == *name) {
            warnings.push(format!("skip_callbacks: '{name}' does not match any defined callback",));
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
