pub(crate) mod actions;
mod context;
pub(crate) mod events;
pub(crate) mod panels;
pub(crate) mod prompt_builder;
pub(crate) mod reverie;
mod run;

pub(crate) use context::{ensure_default_agent, ensure_default_contexts};

use std::sync::mpsc::{Receiver, Sender};

use crate::infra::gh_watcher::GhWatcher;
use crate::infra::tools::{ToolResult, ToolUse};
use crate::infra::watcher::FileWatcher;
use crate::state::State;
use crate::state::cache::CacheUpdate;
use crate::state::persistence::PersistenceWriter;
use crate::ui::help::CommandPalette;
use crate::ui::typewriter::TypewriterBuffer;

/// Deferred `StreamDone` data: (`input_tokens`, `output_tokens`, `cache_hit`, `cache_miss`, `stop_reason`).
pub(crate) type PendingDone = (usize, usize, usize, usize, Option<String>);

/// Reverie stream state — holds the receiver channel for a running reverie.
pub(crate) struct ReverieStream {
    pub rx: Receiver<crate::infra::api::StreamEvent>,
    pub pending_tools: Vec<ToolUse>,
    /// Whether the reverie called Report this turn (to detect missing Report)
    pub report_called: bool,
}

pub(crate) struct App {
    pub state: State,
    pub typewriter: TypewriterBuffer,
    pub pending_done: Option<PendingDone>,
    pub pending_tools: Vec<ToolUse>,
    pub cache_tx: Sender<CacheUpdate>,
    pub file_watcher: Option<FileWatcher>,
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
    /// Separate from `ContextElement.last_refresh_ms` which tracks actual content changes.
    pub last_poll_ms: std::collections::HashMap<String, u64>,
    /// Pending tool results when a question form is blocking (`ask_user_question`)
    pub pending_question_tool_results: Option<Vec<ToolResult>>,
    /// Pending tool results when a console blocking wait is active
    pub pending_console_wait_tool_results: Option<Vec<ToolResult>>,
    /// Accumulated blocking watcher results — collects partial results until ALL blocking watchers complete
    pub accumulated_blocking_results: Vec<cp_base::watchers::WatcherResult>,
    /// Active reverie streams keyed by `agent_id` (one per agent type)
    pub reverie_streams: std::collections::HashMap<String, ReverieStream>,
}

// App impl block is in run/input.rs (primary), with additional methods spread
// across the run/ submodule files.
