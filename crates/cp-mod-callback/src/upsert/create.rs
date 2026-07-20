//! Callback-create path: parse params, write the script file, and register the
//! definition. Split from `tools_upsert.rs` for the line budget; the update /
//! delete paths stay there.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

use globset::Glob;

use cp_base::config::constants;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::{CallbackDefinition, CallbackState};
use std::fmt::Write as _;

/// Validated inputs for a callback-create request.
struct CreateParams {
    /// Callback name (also the script filename stem).
    name: String,
    /// Glob pattern matched against changed file paths.
    pattern: String,
    /// User-supplied script body (below the generated header).
    script: String,
    /// Human-readable description.
    description: String,
    /// Whether the callback blocks the tool pipeline until exit.
    blocking: bool,
    /// Max execution time in seconds (required for blocking callbacks).
    timeout_secs: Option<u64>,
    /// Optional message shown on success (exit 0).
    success_message: Option<String>,
    /// Working directory for the script (defaults to project root).
    cwd: Option<String>,
    /// Global (one invocation, all files) vs local (one per file).
    is_global: bool,
}

/// Extract + validate all `Callback_upsert` create params. Returns the parsed
/// bundle, or an error `ToolResult` (boxed — it is large) on the first
/// invalid/missing field.
fn parse_create_params(tool: &ToolUse) -> Result<CreateParams, Box<ToolResult>> {
    let err = |msg: &str| Err(Box::new(ToolResult::new(tool.id.clone(), msg.to_owned(), true)));

    let Some(name) = tool.input.get("name").and_then(|v| v.as_str()) else {
        return err("Missing required parameter 'name'");
    };
    let Some(pattern) = tool.input.get("pattern").and_then(|v| v.as_str()) else {
        return err("Missing required parameter 'pattern'");
    };
    let Some(script) = tool.input.get("script_content").and_then(|v| v.as_str()) else {
        return err("Missing required parameter 'script_content'");
    };

    if let Err(e) = Glob::new(pattern) {
        return Err(Box::new(ToolResult::new(tool.id.clone(), format!("Invalid glob pattern '{pattern}': {e}"), true)));
    }

    let blocking = tool.input.get("blocking").and_then(serde_json::Value::as_bool).unwrap_or(false);
    let timeout_secs = tool.input.get("timeout").and_then(serde_json::Value::as_u64);
    // Non-blocking callbacks must always be global (dedup requires one-session-per-def).
    let is_global = blocking && tool.input.get("is_global").and_then(serde_json::Value::as_bool).unwrap_or(true);

    if blocking && timeout_secs.is_none() {
        return err("Blocking callbacks require a 'timeout' parameter (max execution time in seconds).");
    }

    if let Err(e) = validate_script_env_vars(script, is_global) {
        return Err(Box::new(ToolResult::new(tool.id.clone(), e, true)));
    }

    Ok(CreateParams {
        name: name.to_owned(),
        pattern: pattern.to_owned(),
        script: script.to_owned(),
        description: tool.input.get("description").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
        blocking,
        timeout_secs,
        success_message: tool.input.get("success_message").and_then(|v| v.as_str()).map(str::to_owned),
        cwd: tool.input.get("cwd").and_then(|v| v.as_str()).map(str::to_owned),
        is_global,
    })
}

/// Write the callback's script file (with the standard env-var header) and mark
/// it executable. Returns a boxed error `ToolResult` on any filesystem failure.
fn write_script_file(tool: &ToolUse, params: &CreateParams) -> Result<(), Box<ToolResult>> {
    let scripts_dir = PathBuf::from(constants::STORE_DIR).join("scripts");
    if let Err(e) = fs::create_dir_all(&scripts_dir) {
        return Err(Box::new(ToolResult::new(
            tool.id.clone(),
            format!("Failed to create scripts directory: {e}"),
            true,
        )));
    }

    let script_path = scripts_dir.join(format!("{}.sh", params.name));
    let full_script = format!(
        "#!/usr/bin/env bash\n\
         set -euo pipefail\n\
         \n\
         # Callback: {name}\n\
         # Pattern: {pattern}\n\
         # Description: {description}\n\
         #\n\
         # Environment variables provided by Context Pilot:\n\
         #   $CP_CHANGED_FILES  — newline-separated list of changed file paths (relative to project root)\n\
         #   $CP_PROJECT_ROOT   — absolute path to project root\n\
         #   $CP_CALLBACK_NAME  — name of this callback rule\n\
         \n\
         {script}",
        name = params.name,
        pattern = params.pattern,
        description = params.description,
        script = params.script,
    );

    if let Err(e) = fs::write(&script_path, &full_script) {
        return Err(Box::new(ToolResult::new(tool.id.clone(), format!("Failed to write script file: {e}"), true)));
    }
    if let Err(e) = fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)) {
        return Err(Box::new(ToolResult::new(tool.id.clone(), format!("Failed to make script executable: {e}"), true)));
    }
    Ok(())
}

/// Build the human-readable success message for a newly created callback.
fn create_success_message(final_id: &str, params: &CreateParams) -> String {
    let mut msg = format!(
        "Created callback {final_id} [{name}]:\n  Pattern: {pattern}\n  Blocking: {blocking}\n  Script: .context-pilot/scripts/{name}.sh",
        name = params.name,
        pattern = params.pattern,
        blocking = params.blocking,
    );
    if let Some(sm) = &params.success_message {
        let _r = write!(msg, "\n  Success message: {sm}");
    }
    if let Some(t) = params.timeout_secs {
        let _r = write!(msg, "\n  Timeout: {t}s");
    }
    let scope = if params.is_global { "global" } else { "local (per-file)" };
    let _r = write!(msg, "\n  Scope: {scope}");
    msg.push_str("\n  Status: active \u{2713}");
    msg
}

/// Create a new callback with its script file.
pub(crate) fn execute_create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let params = match parse_create_params(tool) {
        Ok(p) => p,
        Err(e) => return *e,
    };

    // Check for duplicate name
    if CallbackState::get(state).definitions.iter().any(|d| d.name == params.name) {
        return ToolResult::new(
            tool.id.clone(),
            format!(
                "A callback named '{}' already exists. Use a different name or update the existing one.",
                params.name
            ),
            true,
        );
    }

    if let Err(e) = write_script_file(tool, &params) {
        return *e;
    }

    // Create the definition (id is a placeholder — reassigned by assign_deterministic_ids)
    let definition = CallbackDefinition {
        id: params.name.clone(),
        name: params.name.clone(),
        description: params.description.clone(),
        pattern: params.pattern.clone(),
        blocking: params.blocking,
        timeout_secs: params.timeout_secs,
        success_message: params.success_message.clone(),
        cwd: params.cwd.clone(),
        is_global: params.is_global,
        built_in: false,
        built_in_command: None,
    };

    let cs_store = CallbackState::get_mut(state);
    cs_store.definitions.push(definition);
    cs_store.assign_deterministic_ids();

    // Sync to YAML backing store
    if let Some(created) = CallbackState::get(state).definitions.iter().find(|d| d.name == params.name) {
        crate::storage::upsert_yaml_entry(created);
    }

    // Look up the newly-assigned deterministic ID
    let final_id = CallbackState::get(state)
        .find_by_name_or_id(&params.name)
        .map_or_else(|| params.name.clone(), |d| d.id.clone());

    ToolResult::new(tool.id.clone(), create_success_message(&final_id, &params), false)
}

/// Validate that a callback script uses the correct env var for its scope.
/// Global scripts must use `$CP_CHANGED_FILES` (plural), not singular.
/// Local scripts must use `$CP_CHANGED_FILE` (singular), not plural.
fn validate_script_env_vars(script: &str, is_global: bool) -> Result<(), String> {
    if is_global {
        // Check for singular (without trailing S) — but not plural (with S)
        // Match: $CP_CHANGED_FILE followed by non-S char, or ${CP_CHANGED_FILE}
        if script.contains("${CP_CHANGED_FILE}") || has_singular_env_var(script) {
            return Err("Global callbacks should use $CP_CHANGED_FILES (plural), not $CP_CHANGED_FILE (singular). \
                 Global callbacks receive all changed files at once."
                .to_owned());
        }
    } else {
        // Check for plural ($CP_CHANGED_FILES or ${CP_CHANGED_FILES})
        if script.contains("CP_CHANGED_FILES") {
            return Err("Local callbacks should use $CP_CHANGED_FILE (singular), not $CP_CHANGED_FILES (plural). \
                 Local callbacks fire once per file and receive one file path."
                .to_owned());
        }
    }
    Ok(())
}

/// Check if script contains `$CP_CHANGED_FILE` (singular) without a trailing `S`.
/// Avoids false positives on `$CP_CHANGED_FILES`.
fn has_singular_env_var(script: &str) -> bool {
    let needle = "$CP_CHANGED_FILE";
    let mut start = 0;
    while let Some(pos) = script.get(start..).unwrap_or("").find(needle) {
        let abs_pos = start.saturating_add(pos).saturating_add(needle.len());
        // If the next char is 'S' or 's', this is actually $CP_CHANGED_FILES — skip it
        match script.as_bytes().get(abs_pos) {
            Some(b'S' | b's') => {
                start = abs_pos;
            }
            _ => return true,
        }
    }
    false
}
