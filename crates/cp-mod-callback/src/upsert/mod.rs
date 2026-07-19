use std::fs;
use std::path::PathBuf;

use globset::Glob;

use cp_base::config::constants;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::{CallbackDefinition, CallbackState};

/// Callback-create path (parse params, write script, register), split out.
mod create;
pub(crate) use create::execute_create;

/// Apply the string-valued metadata fields (`name`/`description`/`pattern`/
/// `cwd`) onto `def`, recording each change. Returns an error `ToolResult` on
/// an invalid glob pattern.
fn apply_string_fields(tool: &ToolUse, def: &mut CallbackDefinition, changes: &mut Vec<String>) -> Option<ToolResult> {
    if let Some(name) = tool.input.get("name").and_then(|v| v.as_str()) {
        name.clone_into(&mut def.name);
        changes.push(format!("name → {name}"));
    }
    if let Some(desc) = tool.input.get("description").and_then(|v| v.as_str()) {
        desc.clone_into(&mut def.description);
        changes.push("description updated".to_owned());
    }
    if let Some(pattern) = tool.input.get("pattern").and_then(|v| v.as_str()) {
        if let Err(e) = Glob::new(pattern) {
            return Some(ToolResult::new(tool.id.clone(), format!("Invalid glob pattern '{pattern}': {e}"), true));
        }
        pattern.clone_into(&mut def.pattern);
        changes.push(format!("pattern → {pattern}"));
    }
    if let Some(msg) = tool.input.get("success_message").and_then(|v| v.as_str()) {
        def.success_message = Some(msg.to_owned());
        changes.push("success_message updated".to_owned());
    }
    if let Some(cwd) = tool.input.get("cwd").and_then(|v| v.as_str()) {
        def.cwd = Some(cwd.to_owned());
        changes.push(format!("cwd → {cwd}"));
    }
    None
}

/// Apply the flag/scalar metadata fields (`blocking`/`timeout`/`is_global`)
/// onto `def`, recording each change, then enforce the non-blocking-is-global
/// invariant.
fn apply_flag_fields(tool: &ToolUse, def: &mut CallbackDefinition, changes: &mut Vec<String>) {
    if let Some(blocking) = tool.input.get("blocking").and_then(serde_json::Value::as_bool) {
        def.blocking = blocking;
        changes.push(format!("blocking → {blocking}"));
    }
    if let Some(timeout) = tool.input.get("timeout").and_then(serde_json::Value::as_u64) {
        def.timeout_secs = Some(timeout);
        changes.push(format!("timeout → {timeout}s"));
    }
    if let Some(is_global) = tool.input.get("is_global").and_then(serde_json::Value::as_bool) {
        def.is_global = is_global;
        changes.push(format!("scope → {}", if is_global { "global" } else { "local" }));
    }
    // Non-blocking callbacks must always be global (dedup requires one-session-per-def).
    if !def.blocking && !def.is_global {
        def.is_global = true;
        changes.push("scope → global (forced: non-blocking callbacks must be global)".to_owned());
    }
}

/// Apply all provided metadata fields onto `def`, recording each change.
/// Returns an error `ToolResult` on an invalid glob pattern.
fn apply_metadata_updates(
    tool: &ToolUse,
    def: &mut CallbackDefinition,
    changes: &mut Vec<String>,
) -> Option<ToolResult> {
    if let Some(err) = apply_string_fields(tool, def, changes) {
        return Some(err);
    }
    apply_flag_fields(tool, def, changes);
    None
}

/// Header identity (`name` + `pattern`) baked into a regenerated script file.
struct ScriptMeta<'meta> {
    /// Callback name written into the `# Callback:` header line.
    name: &'meta str,
    /// Glob pattern written into the `# Pattern:` header line.
    pattern: &'meta str,
}

/// Overwrite the script file with a freshly-generated header + `script` body.
fn apply_full_script(
    tool: &ToolUse,
    meta: &ScriptMeta<'_>,
    script_path: &std::path::Path,
    changes: &mut Vec<String>,
) -> Option<ToolResult> {
    let Some(script) = tool.input.get("script_content").and_then(|v| v.as_str()) else {
        return Some(ToolResult::new(tool.id.clone(), "Missing 'script_content' parameter".to_owned(), true));
    };
    let full_script = format!(
        "#!/usr/bin/env bash\nset -euo pipefail\n\n# Callback: {name}\n# Pattern: {pattern}\n\n{script}",
        name = meta.name,
        pattern = meta.pattern,
    );
    if let Err(e) = fs::write(script_path, &full_script) {
        return Some(ToolResult::new(tool.id.clone(), format!("Failed to write script: {e}"), true));
    }
    changes.push("script replaced".to_owned());
    None
}

/// Apply a single `old_string` → `new_string` replacement to the script file.
fn apply_diff_script(tool: &ToolUse, script_path: &std::path::Path, changes: &mut Vec<String>) -> Option<ToolResult> {
    let Some(old_str) = tool.input.get("old_string").and_then(|v| v.as_str()) else {
        return Some(ToolResult::new(tool.id.clone(), "Missing 'old_string' parameter".to_owned(), true));
    };
    let new_str = tool.input.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
    let current = match fs::read_to_string(script_path) {
        Ok(s) => s,
        Err(e) => return Some(ToolResult::new(tool.id.clone(), format!("Failed to read script file: {e}"), true)),
    };
    if !current.contains(old_str) {
        return Some(ToolResult::new(
            tool.id.clone(),
            "old_string not found in script file. Use Callback_open_editor to view current content.".to_owned(),
            true,
        ));
    }
    if let Err(e) = fs::write(script_path, current.replacen(old_str, new_str, 1)) {
        return Some(ToolResult::new(tool.id.clone(), format!("Failed to write script: {e}"), true));
    }
    changes.push("script edited (diff)".to_owned());
    None
}

/// Update an existing callback (full replace or diff-based script edit).
pub(crate) fn execute_update(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(key) = tool.input.get("id").and_then(|v| v.as_str()).map(str::to_owned) else {
        return ToolResult::new(tool.id.clone(), "Missing required parameter 'id' for update action".to_owned(), true);
    };

    let cs = CallbackState::get(state);
    let Some(def_idx) = cs.position_by_name_or_id(&key) else {
        return ToolResult::new(tool.id.clone(), format!("Callback '{key}' not found"), true);
    };
    let Some(matched_def) = cs.definitions.get(def_idx) else {
        return ToolResult::new(tool.id.clone(), format!("Definition index {def_idx} out of bounds"), true);
    };
    let def_name = matched_def.name.clone();
    let def_id = matched_def.id.clone();

    let has_diff = tool.input.get("old_string").and_then(|v| v.as_str()).is_some();
    let has_full_script = tool.input.get("script_content").and_then(|v| v.as_str()).is_some();

    if has_diff && has_full_script {
        return ToolResult::new(
            tool.id.clone(),
            "Cannot use both 'script_content' and 'old_string'/'new_string' in the same update. Use one or the other."
                .to_owned(),
            true,
        );
    }

    // Diff-based edits require the editor open first (so the AI saw current content).
    if has_diff && CallbackState::get(state).editor_open.as_deref() != Some(&def_name) {
        return ToolResult::new(
            tool.id.clone(),
            format!(
                "Diff-based script editing requires the editor to be open. Use Callback_open_editor with id='{key}' first to view current script content."
            ),
            true,
        );
    }

    let cs_mut = CallbackState::get_mut(state);
    let Some(def) = cs_mut.definitions.get_mut(def_idx) else {
        return ToolResult::new(tool.id.clone(), format!("Definition index {def_idx} out of bounds"), true);
    };
    let vessel_name = def.name.clone();
    let mut changes = Vec::new();

    if let Some(err) = apply_metadata_updates(tool, def, &mut changes) {
        return err;
    }

    let scripts_dir = PathBuf::from(constants::STORE_DIR).join("scripts");
    let script_path = scripts_dir.join(format!("{vessel_name}.sh"));
    let (updated_name, updated_pattern) = (def.name.clone(), def.pattern.clone());
    let script_err = if has_full_script {
        apply_full_script(
            tool,
            &ScriptMeta { name: &updated_name, pattern: &updated_pattern },
            &script_path,
            &mut changes,
        )
    } else if has_diff {
        apply_diff_script(tool, &script_path, &mut changes)
    } else {
        None
    };
    if let Some(err) = script_err {
        return err;
    }

    // Handle name rename (move script file)
    if let Some(new_name) = tool.input.get("name").and_then(|v| v.as_str())
        && new_name != vessel_name
    {
        let old_path = scripts_dir.join(format!("{vessel_name}.sh"));
        let new_path = scripts_dir.join(format!("{new_name}.sh"));
        if old_path.exists() {
            let _renamed = fs::rename(&old_path, &new_path);
        }
    }

    if changes.is_empty() {
        return ToolResult::new(tool.id.clone(), format!("Callback {def_id} updated (no changes specified)"), false);
    }

    // Sync to YAML backing store
    if let Some(updated) = CallbackState::get(state).definitions.iter().find(|d| d.name == def_name) {
        crate::storage::upsert_yaml_entry(updated);
    }

    ToolResult::new(tool.id.clone(), format!("Callback {} updated:\n  {}", def_id, changes.join("\n  ")), false)
}

/// Delete a callback and its script file.
pub(crate) fn execute_delete(tool: &ToolUse, state: &mut State) -> ToolResult {
    let key = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_owned(),
        None => {
            return ToolResult::new(
                tool.id.clone(),
                "Missing required parameter 'id' for delete action".to_owned(),
                true,
            );
        }
    };

    let cs = CallbackState::get(state);
    let Some(def_idx) = cs.position_by_name_or_id(&key) else {
        return ToolResult::new(tool.id.clone(), format!("Callback '{key}' not found"), true);
    };
    let Some(def_ref) = cs.definitions.get(def_idx) else {
        return ToolResult::new(tool.id.clone(), format!("Callback '{key}' not found (index out of bounds)"), true);
    };
    let def_id = def_ref.id.clone();

    // Remove definition and get the name for script cleanup
    let cs_mut = CallbackState::get_mut(state);
    let sunken_def = cs_mut.definitions.remove(def_idx);

    // If editor was open for this callback, close it
    if cs_mut.editor_open.as_deref() == Some(sunken_def.name.as_str()) {
        cs_mut.editor_open = None;
    }

    // Reassign deterministic IDs after removal
    cs_mut.assign_deterministic_ids();

    // Remove from YAML backing store
    crate::storage::remove_yaml_entry(&sunken_def.name);

    // Delete the script file
    let script_path = PathBuf::from(constants::STORE_DIR).join("scripts").join(format!("{}.sh", sunken_def.name));
    let script_deleted = if script_path.exists() {
        match fs::remove_file(&script_path) {
            Ok(()) => true,
            Err(e) => {
                return ToolResult::new(
                    tool.id.clone(),
                    format!(
                        "Callback {def_id} [{}] removed from config, but failed to delete script: {}",
                        sunken_def.name, e
                    ),
                    false,
                );
            }
        }
    } else {
        false
    };

    let script_msg = if script_deleted { " + script file deleted" } else { " (no script file found)" };

    ToolResult::new(tool.id.clone(), format!("Callback {def_id} [{}] deleted{}", sunken_def.name, script_msg), false)
}
