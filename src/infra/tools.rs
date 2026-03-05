pub(crate) use cp_base::tools::pre_flight::Verdict;
pub(crate) use cp_base::tools::{ParamType, ToolDefinition, ToolParam, ToolResult, ToolTexts, ToolUse, build_api};

use crate::state::State;

// Re-export from conversation module for backwards compatibility
pub(crate) use crate::modules::conversation::refresh::refresh_conversation_context;

/// Execute a tool and return the result.
/// Delegates to the module system for dispatch.
pub(crate) fn execute_tool(tool: &ToolUse, state: &mut State) -> ToolResult {
    let active_modules = state.active_modules.clone();
    crate::modules::dispatch_tool(tool, state, &active_modules)
}

/// Execute `reload_tui` tool (public for module access)
pub(crate) fn execute_reload_tui(tool: &ToolUse, state: &mut State) -> ToolResult {
    // Set flag - actual reload happens in app.rs after tool result is saved
    state.flags.lifecycle.reload_pending = true;

    ToolResult::new(tool.id.clone(), "Reload initiated. Restarting TUI...".to_string(), false)
}

/// Write `reload_requested: true` into config.json so `run.sh` restarts the TUI.
///
/// Called by the main event loop AFTER `save_state()` — otherwise `save_state`
/// overwrites the flag back to `false`. The main loop already handles terminal
/// cleanup and state persistence; this just sets the restart trigger.
pub(crate) fn write_reload_flag() {
    use std::fs;
    let config_path = ".context-pilot/config.json";

    if let Ok(json) = fs::read_to_string(config_path) {
        let updated = if json.contains("\"reload_requested\":") {
            json.replace("\"reload_requested\": false", "\"reload_requested\": true")
                .replace("\"reload_requested\":false", "\"reload_requested\":true")
        } else {
            let mut s = json.trim_end().trim_end_matches('}').to_string();
            s.push_str(",\n  \"reload_requested\": true\n}");
            s
        };
        let _r = fs::write(config_path, updated);
    }
}
