pub(crate) use cp_base::tools::*;
pub(crate) use cp_base::tools::{ToolResult, ToolUse};

use crate::state::State;

// Re-export from conversation module for backwards compatibility
pub(crate) use crate::modules::conversation::refresh::refresh_conversation_context;

/// Execute a tool and return the result.
/// Delegates to the module system for dispatch.
pub(crate) fn execute_tool(tool: &ToolUse, state: &mut State) -> ToolResult {
    let active_modules = state.active_modules.clone();
    crate::modules::dispatch_tool(tool, state, &active_modules)
}

/// Execute reload_tui tool (public for module access)
pub(crate) fn execute_reload_tui(tool: &ToolUse, state: &mut State) -> ToolResult {
    // Set flag - actual reload happens in app.rs after tool result is saved
    state.reload_pending = true;

    ToolResult::new(tool.id.clone(), "Reload initiated. Restarting TUI...".to_string(), false)
}

/// Perform the actual TUI reload (called from app.rs after tool result is saved)
pub(crate) fn perform_reload(state: &mut State) {
    use crate::state::persistence::save_state;
    use crossterm::{
        execute,
        terminal::{LeaveAlternateScreen, disable_raw_mode},
    };
    use std::fs;
    use std::io::stdout;

    let config_path = ".context-pilot/config.json";

    // Save state before exiting
    save_state(state);

    // Read config, set reload_requested to true, and save
    if let Ok(json) = fs::read_to_string(config_path) {
        // Simple string replacement to set reload_requested: true
        let updated = if json.contains("\"reload_requested\":") {
            json.replace("\"reload_requested\": false", "\"reload_requested\": true")
                .replace("\"reload_requested\":false", "\"reload_requested\":true")
        } else {
            // Add the field before the final }
            json.trim_end().trim_end_matches('}').to_string() + ",\n  \"reload_requested\": true\n}"
        };
        let _r = fs::write(config_path, updated);
    }

    // Clean up terminal
    let _r = disable_raw_mode();
    let _r = execute!(stdout(), LeaveAlternateScreen);

    // Exit - the run.sh supervisor will see reload_requested and restart
    std::process::exit(0);
}
