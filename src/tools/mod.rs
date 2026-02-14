use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state::State;

// Re-export from core module for backwards compatibility
pub use crate::modules::core::conversation::refresh_conversation_context;

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
            // Parse JSON, update field, and serialize back
            match serde_json::from_str::<Value>(&json) {
                Ok(mut config) => {
                    // Set reload_requested to true
                    if let Some(obj) = config.as_object_mut() {
                        obj.insert("reload_requested".to_string(), Value::Bool(true));
                    }
                    
                    // Write back with pretty formatting
                    if let Ok(updated) = serde_json::to_string_pretty(&config) {
                        let _ = fs::write(config_path, updated);
                    }
                }
                Err(e) => {
                    // If JSON parsing fails, fall back to string replacement
                    // This maintains backwards compatibility with malformed configs
                    eprintln!("Warning: Config file is not valid JSON ({}), using fallback string replacement. Consider fixing the config file.", e);
                    let updated = if json.contains("\"reload_requested\":") {
                        json.replace("\"reload_requested\": false", "\"reload_requested\": true")
                            .replace("\"reload_requested\":false", "\"reload_requested\":true")
                    } else {
                        json.trim_end().trim_end_matches('}').to_string()
                            + ",\n  \"reload_requested\": true\n}"
                    };
                    let _ = fs::write(config_path, updated);
                }
            }
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
