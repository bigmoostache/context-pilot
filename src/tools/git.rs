use std::process::Command;

use crate::state::State;
use super::{ToolUse, ToolResult};

/// Execute toggle_git_details tool
pub fn execute_toggle_details(tool: &ToolUse, state: &mut State) -> ToolResult {
    let show = tool.input.get("show")
        .and_then(|v| v.as_bool());

    // Toggle or set explicitly
    let new_value = match show {
        Some(v) => v,
        None => !state.git_show_diffs, // Toggle if not specified
    };

    state.git_show_diffs = new_value;

    // Mark git context as needing refresh so content updates
    for ctx in &mut state.context {
        if ctx.context_type == crate::state::ContextType::Git {
            ctx.cache_deprecated = true;
            break;
        }
    }

    let status = if new_value { "enabled" } else { "disabled" };
    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Git diff details {}", status),
        is_error: false,
    }
}

/// Execute git_commit tool
pub fn execute_commit(tool: &ToolUse, _state: &mut State) -> ToolResult {
    let message = match tool.input.get("message").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Error: 'message' parameter is required".to_string(),
                is_error: true,
            };
        }
    };

    let files: Vec<String> = tool.input.get("files")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    // Check if we're in a git repo
    let repo_check = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output();

    match repo_check {
        Ok(output) if !output.status.success() => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Error: Not a git repository".to_string(),
                is_error: true,
            };
        }
        Err(e) => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Error: Failed to run git: {}", e),
                is_error: true,
            };
        }
        _ => {}
    }

    // Stage files if provided
    if !files.is_empty() {
        let mut add_cmd = Command::new("git");
        add_cmd.arg("add").args(&files);

        match add_cmd.output() {
            Ok(output) if !output.status.success() => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Error staging files: {}", stderr),
                    is_error: true,
                };
            }
            Err(e) => {
                return ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Error running git add: {}", e),
                    is_error: true,
                };
            }
            _ => {}
        }
    }

    // Check if there are staged changes
    let diff_check = Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .output();

    match diff_check {
        Ok(output) if output.status.success() => {
            // Exit code 0 means no staged changes
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Error: No changes staged for commit".to_string(),
                is_error: true,
            };
        }
        Err(e) => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Error checking staged changes: {}", e),
                is_error: true,
            };
        }
        _ => {} // Exit code 1 means there are changes
    }

    // Get stats before committing
    let stats = get_commit_stats();

    // Create the commit
    let commit_result = Command::new("git")
        .args(["commit", "-m", message])
        .output();

    match commit_result {
        Ok(output) if output.status.success() => {
            // Get the commit hash
            let hash = Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let mut result = format!("Committed: {}\n", hash);
            result.push_str(&format!("Message: {}\n", message));

            if let Some((files_changed, insertions, deletions)) = stats {
                result.push_str(&format!("\n{} file(s) changed", files_changed));
                if insertions > 0 {
                    result.push_str(&format!(", +{} insertions", insertions));
                }
                if deletions > 0 {
                    result.push_str(&format!(", -{} deletions", deletions));
                }
            }

            ToolResult {
                tool_use_id: tool.id.clone(),
                content: result,
                is_error: false,
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Error committing: {}{}", stderr, stdout),
                is_error: true,
            }
        }
        Err(e) => {
            ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Error running git commit: {}", e),
                is_error: true,
            }
        }
    }
}

/// Get stats for staged changes before commit
fn get_commit_stats() -> Option<(usize, usize, usize)> {
    let output = Command::new("git")
        .args(["diff", "--cached", "--numstat"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let content = String::from_utf8_lossy(&output.stdout);
    let mut files_changed = 0;
    let mut insertions = 0;
    let mut deletions = 0;

    for line in content.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 2 {
            files_changed += 1;
            // Binary files show "-" for counts
            if let Ok(add) = parts[0].parse::<usize>() {
                insertions += add;
            }
            if let Ok(del) = parts[1].parse::<usize>() {
                deletions += del;
            }
        }
    }

    Some((files_changed, insertions, deletions))
}
