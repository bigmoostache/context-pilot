use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use globset::Glob;

use cp_base::config::constants::STORE_DIR;
use cp_base::state::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::{CallbackDefinition, CallbackState};

/// Create a new callback with its script file.
pub(crate) fn execute_create(tool: &ToolUse, state: &mut State) -> ToolResult {
    // Extract required params
    let vessel_name = match tool.input.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => {
            return ToolResult::new(tool.id.clone(), "Missing required parameter 'name'".to_string(), true);
        }
    };

    let chart_pattern = match tool.input.get("pattern").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => {
            return ToolResult::new(tool.id.clone(), "Missing required parameter 'pattern'".to_string(), true);
        }
    };

    let cargo_script = match tool.input.get("script_content").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return ToolResult::new(tool.id.clone(), "Missing required parameter 'script_content'".to_string(), true);
        }
    };

    // Validate glob pattern compiles
    if let Err(e) = Glob::new(&chart_pattern) {
        return ToolResult::new(tool.id.clone(), format!("Invalid glob pattern '{}': {}", chart_pattern, e), true);
    }

    // Extract optional params
    let description = tool.input.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let blocking = tool.input.get("blocking").and_then(|v| v.as_bool()).unwrap_or(false);
    let timeout_secs = tool.input.get("timeout").and_then(|v| v.as_u64());
    let success_message = tool.input.get("success_message").and_then(|v| v.as_str()).map(|s| s.to_string());
    let cwd = tool.input.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let one_at_a_time = tool.input.get("one_at_a_time").and_then(|v| v.as_bool()).unwrap_or(false);

    // Blocking callbacks require a timeout
    if blocking && timeout_secs.is_none() {
        return ToolResult::new(
            tool.id.clone(),
            "Blocking callbacks require a 'timeout' parameter (max execution time in seconds).".to_string(),
            true,
        );
    }

    // Check for duplicate name
    let cs = CallbackState::get(state);
    if cs.definitions.iter().any(|d| d.name == vessel_name) {
        return ToolResult::new(
            tool.id.clone(),
            format!(
                "A callback named '{}' already exists. Use a different name or update the existing one.",
                vessel_name
            ),
            true,
        );
    }

    // Generate ID
    let cs = CallbackState::get_mut(state);
    let anchor_id = format!("CB{}", cs.next_id);
    cs.next_id += 1;

    // Write script file to .context-pilot/scripts/{name}.sh
    let scripts_dir = PathBuf::from(STORE_DIR).join("scripts");
    if let Err(e) = fs::create_dir_all(&scripts_dir) {
        return ToolResult::new(tool.id.clone(), format!("Failed to create scripts directory: {}", e), true);
    }

    let script_path = scripts_dir.join(format!("{}.sh", vessel_name));
    let full_script = format!(
        "#!/usr/bin/env bash\n\
         set -euo pipefail\n\
         \n\
         # Callback: {name}\n\
         # Pattern: {pattern}\n\
         # Description: {desc}\n\
         #\n\
         # Environment variables provided by Context Pilot:\n\
         #   $CP_CHANGED_FILES  — newline-separated list of changed file paths (relative to project root)\n\
         #   $CP_PROJECT_ROOT   — absolute path to project root\n\
         #   $CP_CALLBACK_NAME  — name of this callback rule\n\
         \n\
         {script}",
        name = vessel_name,
        pattern = chart_pattern,
        desc = description,
        script = cargo_script,
    );

    if let Err(e) = fs::write(&script_path, &full_script) {
        return ToolResult::new(tool.id.clone(), format!("Failed to write script file: {}", e), true);
    }

    // chmod +x
    if let Err(e) = fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)) {
        return ToolResult::new(tool.id.clone(), format!("Failed to make script executable: {}", e), true);
    }

    // Create the definition
    let definition = CallbackDefinition {
        id: anchor_id.clone(),
        name: vessel_name.clone(),
        description,
        pattern: chart_pattern.clone(),
        blocking,
        timeout_secs,
        success_message: success_message.clone(),
        cwd,
        one_at_a_time,
        built_in: false,
        built_in_command: None,
    };

    // Add to state and mark active
    let cs = CallbackState::get_mut(state);
    cs.definitions.push(definition);
    let _ = cs.active_set.insert(anchor_id.clone());

    // Build success message
    let mut msg = format!(
        "Created callback {} [{}]:\n  Pattern: {}\n  Blocking: {}\n  Script: .context-pilot/scripts/{}.sh",
        anchor_id, vessel_name, chart_pattern, blocking, vessel_name,
    );
    if let Some(ref sm) = success_message {
        msg.push_str(&format!("\n  Success message: {}", sm));
    }
    if let Some(t) = timeout_secs {
        msg.push_str(&format!("\n  Timeout: {}s", t));
    }
    msg.push_str(&format!("\n  One at a time: {}", one_at_a_time));
    msg.push_str("\n  Status: active ✓");

    ToolResult::new(tool.id.clone(), msg, false)
}

/// Update an existing callback (full replace or diff-based script edit).
pub(crate) fn execute_update(tool: &ToolUse, state: &mut State) -> ToolResult {
    let anchor_id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => {
            return ToolResult::new(
                tool.id.clone(),
                "Missing required parameter 'id' for update action".to_string(),
                true,
            );
        }
    };

    let cs = CallbackState::get(state);
    let Some(def_idx) = cs.definitions.iter().position(|d| d.id == anchor_id) else {
        return ToolResult::new(tool.id.clone(), format!("Callback '{}' not found", anchor_id), true);
    };

    // Check for diff-based script update (old_string / new_string)
    let has_diff = tool.input.get("old_string").and_then(|v| v.as_str()).is_some();
    let has_full_script = tool.input.get("script_content").and_then(|v| v.as_str()).is_some();

    if has_diff && has_full_script {
        return ToolResult::new(
            tool.id.clone(),
            "Cannot use both 'script_content' and 'old_string'/'new_string' in the same update. Use one or the other."
                .to_string(),
            true,
        );
    }

    // Diff-based edits require the editor to be open first (so the AI can see current content)
    if has_diff {
        let cs = CallbackState::get(state);
        if cs.editor_open.as_deref() != Some(&anchor_id) {
            return ToolResult::new(
                tool.id.clone(),
                format!(
                    "Diff-based script editing requires the editor to be open. Use Callback_open_editor with id='{}' first to view current script content.",
                    anchor_id
                ),
                true,
            );
        }
    }

    let cs = CallbackState::get_mut(state);
    let def = &mut cs.definitions[def_idx];
    let vessel_name = def.name.clone();
    let mut changes = Vec::new();

    // Update metadata fields if provided
    if let Some(name) = tool.input.get("name").and_then(|v| v.as_str()) {
        def.name = name.to_string();
        changes.push(format!("name → {}", name));
    }
    if let Some(desc) = tool.input.get("description").and_then(|v| v.as_str()) {
        def.description = desc.to_string();
        changes.push("description updated".to_string());
    }
    if let Some(pattern) = tool.input.get("pattern").and_then(|v| v.as_str()) {
        if let Err(e) = Glob::new(pattern) {
            return ToolResult::new(tool.id.clone(), format!("Invalid glob pattern '{}': {}", pattern, e), true);
        }
        def.pattern = pattern.to_string();
        changes.push(format!("pattern → {}", pattern));
    }
    if let Some(blocking) = tool.input.get("blocking").and_then(|v| v.as_bool()) {
        def.blocking = blocking;
        changes.push(format!("blocking → {}", blocking));
    }
    if let Some(timeout) = tool.input.get("timeout").and_then(|v| v.as_u64()) {
        def.timeout_secs = Some(timeout);
        changes.push(format!("timeout → {}s", timeout));
    }
    if let Some(msg) = tool.input.get("success_message").and_then(|v| v.as_str()) {
        def.success_message = Some(msg.to_string());
        changes.push("success_message updated".to_string());
    }
    if let Some(cwd) = tool.input.get("cwd").and_then(|v| v.as_str()) {
        def.cwd = Some(cwd.to_string());
        changes.push(format!("cwd → {}", cwd));
    }
    if let Some(oaat) = tool.input.get("one_at_a_time").and_then(|v| v.as_bool()) {
        def.one_at_a_time = oaat;
        changes.push(format!("one_at_a_time → {}", oaat));
    }

    // Handle script updates
    let scripts_dir = PathBuf::from(STORE_DIR).join("scripts");
    let script_path = scripts_dir.join(format!("{}.sh", vessel_name));

    if has_full_script {
        // Full script replacement
        let cargo_script = tool.input["script_content"].as_str().unwrap();
        let full_script = format!(
            "#!/usr/bin/env bash\nset -euo pipefail\n\n# Callback: {name}\n# Pattern: {pattern}\n\n{script}",
            name = def.name,
            pattern = def.pattern,
            script = cargo_script,
        );
        if let Err(e) = fs::write(&script_path, &full_script) {
            return ToolResult::new(tool.id.clone(), format!("Failed to write script: {}", e), true);
        }
        changes.push("script replaced".to_string());
    } else if has_diff {
        // Diff-based script edit
        let old_str = tool.input["old_string"].as_str().unwrap();
        let new_str = tool.input.get("new_string").and_then(|v| v.as_str()).unwrap_or("");

        let current_script = match fs::read_to_string(&script_path) {
            Ok(s) => s,
            Err(e) => {
                return ToolResult::new(tool.id.clone(), format!("Failed to read script file: {}", e), true);
            }
        };

        if !current_script.contains(old_str) {
            return ToolResult::new(
                tool.id.clone(),
                "old_string not found in script file. Use Callback_open_editor to view current content.".to_string(),
                true,
            );
        }

        let updated_script = current_script.replacen(old_str, new_str, 1);
        if let Err(e) = fs::write(&script_path, &updated_script) {
            return ToolResult::new(tool.id.clone(), format!("Failed to write script: {}", e), true);
        }
        changes.push("script edited (diff)".to_string());
    }

    // Handle name rename (move script file)
    if let Some(new_name) = tool.input.get("name").and_then(|v| v.as_str())
        && new_name != vessel_name
    {
        let old_path = scripts_dir.join(format!("{}.sh", vessel_name));
        let new_path = scripts_dir.join(format!("{}.sh", new_name));
        if old_path.exists() {
            let _ = fs::rename(&old_path, &new_path).ok();
        }
    }

    if changes.is_empty() {
        return ToolResult::new(
            tool.id.clone(),
            format!("Callback {} updated (no changes specified)", anchor_id),
            false,
        );
    }

    ToolResult::new(tool.id.clone(), format!("Callback {} updated:\n  {}", anchor_id, changes.join("\n  ")), false)
}

/// Delete a callback and its script file.
pub(crate) fn execute_delete(tool: &ToolUse, state: &mut State) -> ToolResult {
    let anchor_id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => {
            return ToolResult::new(
                tool.id.clone(),
                "Missing required parameter 'id' for delete action".to_string(),
                true,
            );
        }
    };

    let cs = CallbackState::get(state);
    let Some(def_idx) = cs.definitions.iter().position(|d| d.id == anchor_id) else {
        return ToolResult::new(tool.id.clone(), format!("Callback '{}' not found", anchor_id), true);
    };

    // Remove definition and get the name for script cleanup
    let cs = CallbackState::get_mut(state);
    let sunken_def = cs.definitions.remove(def_idx);
    let _ = cs.active_set.remove(&anchor_id);

    // If editor was open for this callback, close it
    if cs.editor_open.as_deref() == Some(&anchor_id) {
        cs.editor_open = None;
    }

    // Delete the script file
    let script_path = PathBuf::from(STORE_DIR).join("scripts").join(format!("{}.sh", sunken_def.name));
    let script_deleted = if script_path.exists() {
        match fs::remove_file(&script_path) {
            Ok(()) => true,
            Err(e) => {
                return ToolResult::new(
                    tool.id.clone(),
                    format!(
                        "Callback {} [{}] removed from config, but failed to delete script: {}",
                        anchor_id, sunken_def.name, e
                    ),
                    false,
                );
            }
        }
    } else {
        false
    };

    let script_msg = if script_deleted { " + script file deleted" } else { " (no script file found)" };

    ToolResult::new(
        tool.id.clone(),
        format!("Callback {} [{}] deleted{}", anchor_id, sunken_def.name, script_msg),
        false,
    )
}
