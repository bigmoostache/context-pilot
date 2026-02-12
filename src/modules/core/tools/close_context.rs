use std::process::Command;

use crate::tools::{ToolResult, ToolUse};
use crate::state::{ContextType, State};

pub fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let ids = match tool.input.get("ids").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'ids' array parameter".to_string(),
                is_error: true,
            }
        }
    };

    if ids.is_empty() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "Empty 'ids' array".to_string(),
            is_error: true,
        };
    }

    let mut closed: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut not_found: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for id_value in ids {
        let Some(id) = id_value.as_str() else {
            errors.push("Invalid ID (not a string)".to_string());
            continue;
        };

        // Find the context element
        let ctx_idx = state.context.iter().position(|c| c.id == id);

        let Some(idx) = ctx_idx else {
            not_found.push(id.to_string());
            continue;
        };

        let ctx = &state.context[idx];

        match ctx.context_type {
            ContextType::System | ContextType::Tree | ContextType::Conversation | ContextType::Todo | ContextType::Memory | ContextType::Overview | ContextType::Git | ContextType::Scratchpad | ContextType::Library | ContextType::Spine | ContextType::Logs => {
                // Protected - cannot close
                skipped.push(format!("{} (protected)", id));
            }
            ContextType::Skill => {
                let name = ctx.name.clone();
                // Remove from loaded_skill_ids
                if let Some(skill_id) = ctx.skill_prompt_id.clone() {
                    state.loaded_skill_ids.retain(|s| s != &skill_id);
                }
                state.context.remove(idx);
                closed.push(format!("{} (skill: {})", id, name));
            }
            ContextType::ConversationHistory => {
                // Redirect to close_conversation_history tool
                skipped.push(format!("{} — Cannot close conversation history with context_close. Use close_conversation_history instead, which lets you create logs and memories to preserve important information before closing.", id));
            }
            ContextType::GitResult | ContextType::GithubResult => {
                let cmd = ctx.result_command.clone().unwrap_or_default();
                state.context.remove(idx);
                closed.push(format!("{} ({})", id, cmd));
            }
            ContextType::File => {
                let name = ctx.name.clone();
                state.context.remove(idx);
                closed.push(format!("{} (file: {})", id, name));
            }
            ContextType::Glob => {
                let pattern = ctx.glob_pattern.clone().unwrap_or_default();
                state.context.remove(idx);
                closed.push(format!("{} (glob: {})", id, pattern));
            }
            ContextType::Grep => {
                let pattern = ctx.grep_pattern.clone().unwrap_or_default();
                state.context.remove(idx);
                closed.push(format!("{} (grep: {})", id, pattern));
            }
            ContextType::Tmux => {
                let pane_id = ctx.tmux_pane_id.clone();
                let desc = ctx.tmux_description.clone().unwrap_or_default();

                // Kill the tmux window
                if let Some(pane) = &pane_id {
                    let output = Command::new("tmux")
                        .args(["kill-window", "-t", pane])
                        .output();

                    match output {
                        Ok(out) if out.status.success() => {
                            state.context.remove(idx);
                            closed.push(format!("{} (tmux: {})", id, desc));
                        }
                        Ok(out) => {
                            // Still remove from context even if kill failed
                            state.context.remove(idx);
                            errors.push(format!("{}: tmux kill failed: {}", id,
                                String::from_utf8_lossy(&out.stderr).trim()));
                        }
                        Err(e) => {
                            state.context.remove(idx);
                            errors.push(format!("{}: tmux error: {}", id, e));
                        }
                    }
                } else {
                    state.context.remove(idx);
                    closed.push(format!("{} (tmux: {})", id, desc));
                }
            }
        }
    }

    // Build response
    let mut output = String::new();

    if !closed.is_empty() {
        output.push_str(&format!("Closed {}:\n{}", closed.len(), closed.join("\n")));
    }

    if !skipped.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str(&format!("Skipped {}:\n{}", skipped.len(), skipped.join("\n")));
    }

    if !not_found.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str(&format!("Not found: {}", not_found.join(", ")));
    }

    if !errors.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str(&format!("Errors:\n{}", errors.join("\n")));
    }

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: output,
        is_error: closed.is_empty() && skipped.is_empty(),
    }
}
