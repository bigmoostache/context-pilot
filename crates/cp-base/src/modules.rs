use std::process::{Command, Output, Stdio};
use std::time::Duration;

use crate::panels::Panel;
use crate::state::{ContextType, State};
use crate::tool_defs::ToolDefinition;
use crate::tools::{ToolResult, ToolUse};

/// Run a Command with a timeout. Returns TimedOut error if the command exceeds the limit.
pub fn run_with_timeout(mut cmd: Command, timeout_secs: u64) -> std::io::Result<Output> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    let child = cmd.spawn()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(Duration::from_secs(timeout_secs)) {
        Ok(result) => result,
        Err(_) => {
            Err(std::io::Error::new(std::io::ErrorKind::TimedOut, format!("Command timed out after {}s", timeout_secs)))
        }
    }
}

/// Truncate output to max_bytes, respecting UTF-8 char boundaries.
pub fn truncate_output(output: &str, max_bytes: usize) -> String {
    if output.len() <= max_bytes {
        output.to_string()
    } else {
        let truncated = &output[..output.floor_char_boundary(max_bytes)];
        format!("{}\n\n[Output truncated at 1MB]", truncated)
    }
}

/// A module that provides tools, panels, and configuration to the TUI.
///
/// Modules are stateless — all runtime state lives in `State`.
/// Activation/deactivation is a config toggle that controls whether
/// the module's tools and panels are registered.
pub trait Module: Send + Sync {
    /// Unique identifier (e.g., "core", "git", "tmux")
    fn id(&self) -> &'static str;
    /// Display name
    fn name(&self) -> &'static str;
    /// Short description
    fn description(&self) -> &'static str;
    /// IDs of modules this one depends on
    fn dependencies(&self) -> &[&'static str] {
        &[]
    }
    /// Core modules cannot be deactivated
    fn is_core(&self) -> bool {
        false
    }

    /// Whether this module's data is global (config.json) or per-worker (worker.json)
    fn is_global(&self) -> bool {
        false
    }

    /// Initialize module-owned state in the State extension map.
    /// Called once at startup for each module. Use `state.set_ext(MyState { ... })`.
    fn init_state(&self, _state: &mut State) {}

    /// Reset module-owned state to defaults (e.g., when loading a preset).
    /// Called by preset system to clear module data without full restart.
    fn reset_state(&self, _state: &mut State) {}

    /// Serialize this module's data from State into a JSON value for persistence.
    /// Returns Value::Null if this module has no data to persist.
    /// Stored in SharedConfig (if is_global) or WorkerState (if !is_global).
    fn save_module_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    /// Deserialize this module's data from a JSON value and apply it to State.
    /// Data comes from SharedConfig (if is_global) or WorkerState (if !is_global).
    fn load_module_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    /// Serialize worker-specific data for modules that are global but also need per-worker state.
    /// Returns Value::Null if no worker-specific data. Always stored in WorkerState.
    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    /// Deserialize worker-specific data. Always loaded from WorkerState.
    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    /// Tool definitions provided by this module
    fn tool_definitions(&self) -> Vec<ToolDefinition>;
    /// Execute a tool. Returns None if this module doesn't own the tool.
    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult>;

    /// Create a panel for the given context type. Returns None if not owned by this module.
    fn create_panel(&self, context_type: &ContextType) -> Option<Box<dyn Panel>>;

    /// Fixed panel types owned by this module (P0-P7)
    fn fixed_panel_types(&self) -> Vec<ContextType> {
        vec![]
    }
    /// Dynamic panel types this module can create (File, Glob, Grep, Tmux)
    fn dynamic_panel_types(&self) -> Vec<ContextType> {
        vec![]
    }

    /// Default settings for fixed panels: (context_type, display_name, cache_deprecated).
    /// Used by ensure_default_contexts to create missing panels generically.
    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> {
        vec![]
    }
}
