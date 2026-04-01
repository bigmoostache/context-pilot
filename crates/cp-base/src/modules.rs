use std::process::{Command, Output, Stdio};
use std::time::Duration;

use crate::panels::Panel;
use crate::state::context::{Kind, TypeMeta};
use crate::state::runtime::State;
use crate::tools::ToolDefinition;
use crate::tools::pre_flight::Verdict;
use crate::tools::{ToolResult, ToolUse};

/// A function that transforms tool result content into IR blocks.
///
/// Receives the raw content string and available display width.
/// Used by modules to register custom visualizations for their tool results.
pub type ToolVisualizer = fn(content: &str, width: usize) -> Vec<cp_render::Block>;

/// Run a Command with a timeout. Returns `TimedOut` error if the command exceeds the limit.
///
/// # Errors
///
/// Returns `std::io::Error` if the command fails to spawn, or
/// `ErrorKind::TimedOut` if execution exceeds `timeout_secs`.
pub fn run_with_timeout(mut cmd: Command, timeout_secs: u64) -> std::io::Result<Output> {
    _ = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    let child = cmd.spawn()?;
    let (tx, rx) = std::sync::mpsc::channel();
    drop(std::thread::spawn(move || {
        let _r = tx.send(child.wait_with_output());
    }));
    rx.recv_timeout(Duration::from_secs(timeout_secs)).unwrap_or_else(|_| {
        Err(std::io::Error::new(std::io::ErrorKind::TimedOut, format!("Command timed out after {timeout_secs}s")))
    })
}

/// Truncate output to `max_bytes`, respecting UTF-8 char boundaries.
#[must_use]
pub fn truncate_output(output: &str, max_bytes: usize) -> String {
    if output.len() <= max_bytes {
        output.to_string()
    } else {
        let truncated = &output.get(..output.floor_char_boundary(max_bytes)).unwrap_or("");
        format!("{truncated}\n\n[Output truncated at 1MB]")
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
    /// Returns `Value::Null` if this module has no data to persist.
    /// Stored in `config::Shared` (if `is_global`) or `WorkerState` (if !`is_global`).
    fn save_module_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    /// Deserialize this module's data from a JSON value and apply it to State.
    /// Data comes from `config::Shared` (if `is_global`) or `WorkerState` (if !`is_global`).
    fn load_module_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    /// Serialize worker-specific data for modules that are global but also need per-worker state.
    /// Returns `Value::Null` if no worker-specific data. Always stored in `WorkerState`.
    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    /// Deserialize worker-specific data. Always loaded from `WorkerState`.
    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    /// Tool definitions provided by this module
    fn tool_definitions(&self) -> Vec<ToolDefinition>;
    /// Execute a tool. Returns None if this module doesn't own the tool.
    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult>;

    /// Pre-flight validation for a tool call. Runs BEFORE execution (and before
    /// queueing). Returns `None` if this module doesn't own the tool.
    /// Returns `Some(Verdict)` with errors (block execution) and/or
    /// warnings (pass-through, tool still runs). Default: no custom checks.
    fn pre_flight(&self, _tool: &ToolUse, _state: &State) -> Option<Verdict> {
        None
    }

    /// Create a panel for the given context type. Returns None if not owned by this module.
    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>>;

    /// Fixed panel types owned by this module (P0-P7)
    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![]
    }
    /// Dynamic panel types this module can create (File, Glob, Grep, Tmux)
    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    /// Default settings for fixed panels: (`context_type`, `display_name`, `cache_deprecated`).
    /// Used by `ensure_default_contexts` to create missing panels generically.
    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![]
    }

    /// Context type metadata for the registry.
    /// Each module declares its owned context types with icon, fixed/cache flags, and sort order.
    fn context_type_metadata(&self) -> Vec<TypeMeta> {
        vec![]
    }

    /// Return tool result visualizers owned by this module.
    /// Each entry maps a `tool_id` to a function that transforms the tool result
    /// content into styled terminal lines. If no visualizer is registered for a
    /// tool, the core renderer falls back to plain text display.
    ///
    /// The visualizer receives the raw result content string and the available
    /// display width, and returns colored/styled `Line`s for the TUI.
    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![]
    }

    // === Overview delegation ===
    // These methods let modules provide their own overview content instead of
    // hardcoding module knowledge in the binary's overview panel.

    /// Return display name for a context type owned by this module (e.g., "todo" → "wip").
    /// Used in the Overview panel's context elements table.
    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
        None
    }

    /// Return detail string for a context element owned by this module (e.g., file path, pattern).
    /// Used in the Overview panel's context elements table.
    fn context_detail(&self, _ctx: &crate::state::context::Entry) -> Option<String> {
        None
    }

    /// Return LLM-facing overview text for this module's state (e.g., "Todos: 3/5 done").
    /// Appended to the Overview context sent to the LLM.
    fn overview_context_section(&self, _state: &State) -> Option<String> {
        None
    }

    /// Return TUI-rendered overview section(s) for this module.
    /// Each element is (`section_order`, `rendered_blocks`). Sections are sorted by order.
    fn overview_render_sections(&self, _state: &State) -> Vec<(u8, Vec<cp_render::Block>)> {
        vec![]
    }

    /// Handle closing a context element of this module's type.
    /// Returns None if this module doesn't own the context type.
    /// Returns Some(Err(message)) if the close should be blocked/redirected.
    /// Returns Some(Ok(description)) if cleanup succeeded — caller removes the context element.
    fn on_close_context(
        &self,
        _ctx: &crate::state::context::Entry,
        _state: &mut State,
    ) -> Option<Result<String, String>> {
        None
    }

    /// Return tool category descriptions for tools owned by this module.
    /// Each entry is (`category_id`, description). Used in the Overview panel's tool listing.
    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![]
    }

    // === Lifecycle hooks ===

    /// Called when the user submits a message (before streaming starts).
    /// Modules can reset counters, create notifications, etc.
    fn on_user_message(&self, _state: &mut State) {}

    /// Called when streaming is stopped by the user (Esc key).
    /// Modules can update their state to reflect the stop.
    fn on_stream_stop(&self, _state: &mut State) {}

    /// Called when a tool call is being streamed (progress update).
    ///
    /// Fires each time the LLM emits more JSON for a tool call.
    /// `tool_name` is resolved early; `input_so_far` is the partial
    /// JSON of the parameters seen so far (may be incomplete).
    ///
    /// Modules can use this to provide live feedback — e.g. the chat
    /// module sends Matrix typing indicators when it detects a
    /// `Chat_send` with a resolved `room` parameter.
    fn on_tool_progress(&self, _tool_name: &str, _input_so_far: &str, _state: &mut State) {}

    /// Called when a tool call stream completes (full JSON received).
    ///
    /// Counterpart to [`on_tool_progress`](Self::on_tool_progress).
    /// Used to clean up any live-feedback state (e.g. clear typing
    /// indicators).
    fn on_tool_complete(&self, _tool_name: &str, _state: &mut State) {}

    // === File watcher delegation ===

    /// Return filesystem paths this module wants the file watcher to monitor.
    /// Called periodically to sync watchers. Includes both global paths (e.g., .git/)
    /// and per-context-element paths (e.g., individual file paths).
    fn watch_paths(&self, _state: &State) -> Vec<crate::panels::WatchSpec> {
        vec![]
    }

    /// Check if a filesystem change event should invalidate a specific context element.
    /// `is_dir_event`: true for directory changes, false for file changes.
    /// Returns true if the element should be marked `cache_deprecated`.
    fn should_invalidate_on_fs_change(
        &self,
        _ctx: &crate::state::context::Entry,
        _changed_path: &str,
        _is_dir_event: bool,
    ) -> bool {
        false
    }

    /// Whether watcher-triggered invalidation should schedule immediate cache refresh.
    /// If false, invalidation only marks the panel dirty for timer-based refresh.
    /// Default is true. Override to false for modules where immediate refresh would
    /// create feedback loops (e.g., git: `git status` writes `.git/index`).
    fn watcher_immediate_refresh(&self) -> bool {
        true
    }
}
