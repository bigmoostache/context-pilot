use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state::{estimate_tokens, ContextType, MessageStatus, State};

/// Refresh token count for the Conversation context element
pub fn refresh_conversation_context(state: &mut State) {
    // Calculate total tokens from all active messages
    let total_tokens: usize = state.messages.iter()
        .filter(|m| m.status != MessageStatus::Deleted)
        .map(|m| {
            match m.status {
                MessageStatus::Summarized => m.tl_dr_token_count.max(estimate_tokens(m.tl_dr.as_deref().unwrap_or(""))),
                _ => m.content_token_count.max(estimate_tokens(&m.content)),
            }
        })
        .sum();

    // Update the Conversation context element's token count
    for ctx in &mut state.context {
        if ctx.context_type == ContextType::Conversation {
            ctx.token_count = total_tokens;
            break;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUse {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
}

/// Execute a tool and return the result.
/// Delegates to the module system for dispatch.
pub fn execute_tool(tool: &ToolUse, state: &mut State) -> ToolResult {
    let active_modules = state.active_modules.clone();
    crate::modules::dispatch_tool(tool, state, &active_modules)
}

/// Execute reload_tui tool (public for module access)
pub fn execute_reload_tui(tool: &ToolUse, state: &mut State) -> ToolResult {
    // Set flag - actual reload happens in app.rs after tool result is saved
    state.reload_pending = true;

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: "Reload initiated. Restarting TUI...".to_string(),
        is_error: false,
    }
}

/// Perform the actual TUI reload (called from app.rs after tool result is saved)
pub fn perform_reload(state: &mut State) {
    use std::fs;
    use std::io::stdout;
    use crossterm::{execute, terminal::{disable_raw_mode, LeaveAlternateScreen}};
    use crate::persistence::save_state;

    let config_path = ".context-pilot/config.json";

    // Save state before exiting
    save_state(state);

    // Read config, set reload_requested to true, and save
    match fs::read_to_string(config_path) {
        Ok(json) => {
            // Simple string replacement to set reload_requested: true
            let updated = if json.contains("\"reload_requested\":") {
                json.replace("\"reload_requested\": false", "\"reload_requested\": true")
                    .replace("\"reload_requested\":false", "\"reload_requested\":true")
            } else {
                // Add the field before the final }
                json.trim_end().trim_end_matches('}').to_string()
                    + ",\n  \"reload_requested\": true\n}"
            };
            let _ = fs::write(config_path, updated);
        }
        Err(_) => {
            // If we can't read config, just try to reload anyway
        }
    }

    // Clean up terminal
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), LeaveAlternateScreen);

    // Exit - the run.sh supervisor will see reload_requested and restart
    std::process::exit(0);
}
