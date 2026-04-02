/// Application actions and command dispatch.
pub(crate) mod actions;
/// Context management: preparation, detachment, defaults.
mod context;
/// Keyboard/mouse event handling and routing.
pub(crate) mod events;
/// Panel trait bridge: rendering, context collection, registry lookup.
pub(crate) mod panels;
/// Centralized prompt assembly for all LLM providers.
pub(crate) mod prompt_builder;
/// Reverie sub-agent: trigger, tools, and lifecycle.
pub(crate) mod reverie;
/// Main event loop, streaming, tool pipeline, watchers.
pub(crate) mod run;

pub(crate) use context::{ensure_default_agent, ensure_default_contexts};

use std::sync::mpsc::{Receiver, Sender};

use crate::infra::gh_watcher::GhWatcher;
use crate::infra::tools::{ToolResult, ToolUse};
use crate::infra::watcher::FileWatcher;
use crate::state::State;
use crate::state::cache::CacheUpdate;
use crate::state::persistence::PersistenceWriter;
use crate::ui::TypewriterBuffer;
use crate::ui::help::CommandPalette;

/// Deferred `StreamDone` data: (`input_tokens`, `output_tokens`, `cache_hit`, `cache_miss`, `stop_reason`).
pub(crate) type PendingDone = (usize, usize, usize, usize, Option<String>);

/// Reverie stream state — holds the receiver channel for a running reverie.
pub(crate) struct ReverieStream {
    /// Receiver for stream events from the reverie's API call.
    pub rx: Receiver<crate::infra::api::StreamEvent>,
    /// Pending tool calls accumulated during the reverie stream.
    pub pending_tools: Vec<ToolUse>,
    /// Whether the reverie called Report this turn (to detect missing Report)
    pub report_called: bool,
}

/// Top-level application state container for the TUI event loop.
pub(crate) struct App {
    /// Shared runtime state (context, messages, config, flags).
    pub state: State,
    /// Streaming text typewriter buffer for smooth character-by-character output.
    pub typewriter: TypewriterBuffer,
    /// Deferred stream-done data waiting for typewriter drain.
    pub pending_done: Option<PendingDone>,
    /// Pending tool calls accumulated during streaming.
    pub pending_tools: Vec<ToolUse>,
    /// Sender for cache update requests to the background cache thread.
    pub cache_tx: Sender<CacheUpdate>,
    /// Optional file-system watcher for auto-refresh on file changes.
    pub file_watcher: Option<FileWatcher>,
    /// GitHub event watcher for PR/issue notifications.
    pub gh_watcher: GhWatcher,
    /// Tracks which file paths are being watched
    pub watched_file_paths: std::collections::HashSet<String>,
    /// Tracks which directory paths are being watched
    pub watched_dir_paths: std::collections::HashSet<String>,
    /// Last time we checked timer-based caches
    pub last_timer_check_ms: u64,
    /// Last time we checked ownership
    pub last_ownership_check_ms: u64,
    /// Pending retry error (will retry on next loop iteration)
    pub pending_retry_error: Option<String>,
    /// Last render time for throttling
    pub last_render_ms: u64,
    /// Last spinner animation update time
    pub last_spinner_ms: u64,
    /// Last gh watcher sync time
    pub last_gh_sync_ms: u64,
    /// Last Matrix sync drain time — for periodic idle-time event polling
    pub last_chat_drain_ms: u64,
    /// Channel for API check results
    pub api_check_rx: Option<Receiver<crate::llms::ApiCheckResult>>,
    /// Whether to auto-start streaming on first loop iteration
    pub resume_stream: bool,
    /// Command palette state
    pub command_palette: CommandPalette,
    /// Timestamp (ms) when `wait_for_panels` started (for timeout)
    pub wait_started_ms: u64,
    /// Deferred tool results waiting for sleep timer to expire
    pub deferred_tool_sleep_until_ms: u64,
    /// Whether we're in a deferred sleep state (waiting for timer before continuing tool pipeline)
    pub deferred_tool_sleeping: bool,
    /// Background persistence writer — offloads file I/O to a dedicated thread
    pub writer: PersistenceWriter,
    /// Last poll time per panel ID — tracks when we last submitted a cache request
    /// for timer-based panels (Tmux, Git, `GitResult`, `GithubResult`, Glob, Grep).
    /// Separate from `Entry.last_refresh_ms` which tracks actual content changes.
    pub last_poll_ms: std::collections::HashMap<String, u64>,
    /// Pending tool results when a question form is blocking (`ask_user_question`)
    pub pending_question_tool_results: Option<Vec<ToolResult>>,
    /// Pending tool results when a console blocking wait is active
    pub pending_console_wait_tool_results: Option<Vec<ToolResult>>,
    /// Accumulated blocking watcher results — collects partial results until ALL blocking watchers complete
    pub accumulated_blocking_results: Vec<cp_base::state::watchers::WatcherResult>,
    /// Active reverie streams keyed by `agent_id` (one per agent type)
    pub reverie_streams: std::collections::HashMap<String, ReverieStream>,
}

// App impl block is in run/input.rs (primary), with additional methods spread
// across the run/ submodule files.
