//! Async tool execution — spawn blocking I/O on a worker thread.
//!
//! Tools that perform HTTP requests or subprocess calls use
//! [`spawn_async_tool`] to avoid blocking the main event loop.
//! The result is delivered via a [`ChannelWatcher`](crate::state::watchers::ChannelWatcher)
//! and replaces a sentinel in the deferred-result pipeline.

use std::sync::mpsc;

use crate::state::watchers::carriers::{DynPanel, WatcherResult};
use crate::state::watchers::{ASYNC_ERROR_PREFIX, ChannelWatcher, WatcherRegistry};
use crate::tools::{ToolResult, ToolUse};

/// Sentinel value returned by tools that execute asynchronously on a worker thread.
///
/// The main event loop detects this sentinel and defers the tool result until
/// the corresponding [`ChannelWatcher`] fires.
/// Identical to the console module's blocking sentinel for compatibility with
/// the existing deferred-result pipeline in `cleanup.rs`.
pub const BLOCKING_TOOL_SENTINEL: &str = "__CONSOLE_WAIT_BLOCKING__";

/// Output produced by an async tool's worker thread.
///
/// Sent through the channel to [`ChannelWatcher`],
/// which converts it into a [`WatcherResult`] for sentinel replacement.
#[derive(Debug)]
#[non_exhaustive]
pub struct ToolOutput {
    /// Content string that replaces the sentinel in the tool result.
    /// This is what the LLM sees in the conversation.
    pub content: String,
    /// Whether the tool execution failed.
    pub is_error: bool,
    /// If set, a dynamic panel is created when the watcher fires.
    /// Use [`DYN_PANEL_ID_PLACEHOLDER`](crate::state::watchers::DYN_PANEL_ID_PLACEHOLDER)
    /// in `content` as a placeholder — it gets replaced with the actual panel ID.
    pub create_panel: Option<DynPanel>,
    /// When `true`, this result does NOT break tempo.
    pub preserves_tempo: bool,
}

impl ToolOutput {
    /// Successful output: `content` shown to the LLM, no panel, breaks tempo.
    #[must_use]
    pub fn ok<S>(content: S) -> Self
    where
        S: Into<String>,
    {
        Self { content: content.into(), is_error: false, create_panel: None, preserves_tempo: false }
    }

    /// Error output: `content` is the error message shown to the LLM.
    #[must_use]
    pub fn error<S>(content: S) -> Self
    where
        S: Into<String>,
    {
        Self { content: content.into(), is_error: true, create_panel: None, preserves_tempo: false }
    }

    /// Full control over every field (panel creation, tempo preservation).
    #[must_use]
    pub const fn new(content: String, is_error: bool, create_panel: Option<DynPanel>, preserves_tempo: bool) -> Self {
        Self { content, is_error, create_panel, preserves_tempo }
    }

    /// Attach a dynamic panel to create when the watcher fires (builder).
    #[must_use]
    pub fn with_panel(mut self, panel: DynPanel) -> Self {
        self.create_panel = Some(panel);
        self
    }
}

/// Spawn a tool's I/O work on a background thread, returning a sentinel
/// [`ToolResult`] that will be replaced when the work completes.
///
/// The `work` closure runs on a **worker thread without access to `State`**.
/// Extract all needed parameters (API keys, URLs, command args) on the main
/// thread first, then capture them in the closure.
///
/// # Arguments
///
/// * `state` — Mutable state reference (used to register the watcher).
/// * `tool` — The tool invocation (provides `id` and `name`).
/// * `timeout_secs` — Maximum wait time before returning a timeout error.
/// * `work` — Closure that performs the blocking I/O and returns a [`ToolOutput`].
///
/// # Example
///
/// ```ignore
/// let api_key = get_api_key();
/// let query = tool.input["query"].as_str().unwrap().to_string();
/// spawn_async_tool(state, tool, 30, move || {
///     let response = http_get(&api_key, &query);
///     ToolOutput { content: response, is_error: false, create_panel: None, preserves_tempo: false }
/// })
/// ```
pub fn spawn_async_tool<F>(
    state: &mut crate::state::runtime::State,
    tool: &ToolUse,
    timeout_secs: u64,
    work: F,
) -> ToolResult
where
    F: FnOnce() -> ToolOutput + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    let tool_use_id = tool.id.clone();
    let tool_name_clone = tool.name.clone();

    let handle = std::thread::Builder::new().name(format!("async-tool-{}", tool.name)).spawn(move || {
        let output = work();
        // Encode error status in the description prefix since WatcherResult
        // cannot carry an is_error bool (struct_excessive_bools is forbid-level).
        let description =
            if output.is_error { format!("{ASYNC_ERROR_PREFIX}{}", output.content) } else { output.content };
        let mut result = WatcherResult::new(description).tool_use_id(tool_use_id);
        if let Some(panel) = output.create_panel {
            result = result.create_dyn_panel(panel);
        }
        if output.preserves_tempo {
            result = result.preserves_tempo();
        }
        // If send fails, the watcher will detect Disconnected and return an error.
        let _r = tx.send(result);
    });

    if let Err(e) = &handle {
        // Thread spawn failed — return error synchronously
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Failed to spawn worker thread: {e}"),
            display: None,
            tldr: None,
            is_error: true,
            preserves_tempo: false,
            tool_name: tool_name_clone,
        };
    }

    let description = format!("⏳ {}", tool.name);
    let watcher = ChannelWatcher::new(&description, &tool.id, rx, timeout_secs.saturating_mul(1000));
    WatcherRegistry::get_mut(state).register(Box::new(watcher));

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: BLOCKING_TOOL_SENTINEL.to_owned(),
        display: None,
        tldr: None,
        is_error: false,
        preserves_tempo: true, // Deferred — watcher decides
        tool_name: tool.name.clone(),
    }
}
