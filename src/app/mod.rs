pub(crate) mod actions;
mod context;
pub(crate) mod events;
pub(crate) mod panels;
pub(crate) mod prompt_builder;
pub(crate) mod reverie;
mod run;

pub(crate) use context::{ensure_default_agent, ensure_default_contexts};

use std::sync::mpsc::{Receiver, Sender};

use crate::app::panels::now_ms;
use crate::infra::gh_watcher::GhWatcher;
use crate::infra::tools::{ToolResult, ToolUse};
use crate::infra::watcher::FileWatcher;
use crate::state::cache::CacheUpdate;
use crate::state::persistence::{PersistenceWriter, build_message_op, build_save_batch};
use crate::state::{Message, State};
use crate::ui::help::CommandPalette;
use crate::ui::typewriter::TypewriterBuffer;

/// Reverie stream state — holds the receiver channel for a running reverie.
struct ReverieStream {
    rx: Receiver<crate::infra::api::StreamEvent>,
    pending_tools: Vec<ToolUse>,
    /// Whether the reverie called Report this turn (to detect missing Report)
    report_called: bool,
}

pub(crate) struct App {
    pub state: State,
    typewriter: TypewriterBuffer,
    pending_done: Option<(usize, usize, usize, usize, Option<String>)>,
    pending_tools: Vec<ToolUse>,
    cache_tx: Sender<CacheUpdate>,
    file_watcher: Option<FileWatcher>,
    gh_watcher: GhWatcher,
    /// Tracks which file paths are being watched
    watched_file_paths: std::collections::HashSet<String>,
    /// Tracks which directory paths are being watched
    watched_dir_paths: std::collections::HashSet<String>,
    /// Last time we checked timer-based caches
    last_timer_check_ms: u64,
    /// Last time we checked ownership
    last_ownership_check_ms: u64,
    /// Pending retry error (will retry on next loop iteration)
    pending_retry_error: Option<String>,
    /// Last render time for throttling
    last_render_ms: u64,
    /// Last spinner animation update time
    last_spinner_ms: u64,
    /// Last gh watcher sync time
    last_gh_sync_ms: u64,
    /// Channel for API check results
    api_check_rx: Option<Receiver<crate::llms::ApiCheckResult>>,
    /// Whether to auto-start streaming on first loop iteration
    resume_stream: bool,
    /// Command palette state
    pub command_palette: CommandPalette,
    /// Timestamp (ms) when wait_for_panels started (for timeout)
    wait_started_ms: u64,
    /// Deferred tool results waiting for sleep timer to expire
    deferred_tool_sleep_until_ms: u64,
    /// Whether we're in a deferred sleep state (waiting for timer before continuing tool pipeline)
    deferred_tool_sleeping: bool,
    /// Background persistence writer — offloads file I/O to a dedicated thread
    writer: PersistenceWriter,
    /// Last poll time per panel ID — tracks when we last submitted a cache request
    /// for timer-based panels (Tmux, Git, GitResult, GithubResult, Glob, Grep).
    /// Separate from ContextElement.last_refresh_ms which tracks actual content changes.
    last_poll_ms: std::collections::HashMap<String, u64>,
    /// Pending tool results when a question form is blocking (ask_user_question)
    pending_question_tool_results: Option<Vec<ToolResult>>,
    /// Pending tool results when a console blocking wait is active
    pending_console_wait_tool_results: Option<Vec<ToolResult>>,
    /// Accumulated blocking watcher results — collects partial results until ALL blocking watchers complete
    accumulated_blocking_results: Vec<cp_base::watchers::WatcherResult>,
    /// Active reverie streams keyed by agent_id (one per agent type)
    reverie_streams: std::collections::HashMap<String, ReverieStream>,
}

impl App {
    pub(crate) fn new(state: State, cache_tx: Sender<CacheUpdate>, resume_stream: bool) -> Self {
        let file_watcher = FileWatcher::new().ok();
        let gh_watcher = GhWatcher::new(cache_tx.clone());

        Self {
            state,
            typewriter: TypewriterBuffer::new(),
            pending_done: None,
            pending_tools: Vec::new(),
            cache_tx,
            file_watcher,
            gh_watcher,
            watched_file_paths: std::collections::HashSet::new(),
            watched_dir_paths: std::collections::HashSet::new(),
            last_timer_check_ms: now_ms(),
            last_ownership_check_ms: now_ms(),
            pending_retry_error: None,
            last_render_ms: 0,
            last_spinner_ms: 0,
            last_gh_sync_ms: 0,
            api_check_rx: None,
            resume_stream,
            command_palette: CommandPalette::new(),
            wait_started_ms: 0,
            deferred_tool_sleep_until_ms: 0,
            deferred_tool_sleeping: false,
            writer: PersistenceWriter::new(),
            last_poll_ms: std::collections::HashMap::new(),
            pending_question_tool_results: None,
            pending_console_wait_tool_results: None,
            accumulated_blocking_results: Vec::new(),
            reverie_streams: std::collections::HashMap::new(),
        }
    }

    /// Send state to background writer (debounced, non-blocking).
    /// Preferred over `save_state()` in the main event loop.
    fn save_state_async(&self) {
        self.writer.send_batch(build_save_batch(&self.state));
    }

    /// Send a message to background writer (non-blocking).
    /// Preferred over `save_message()` in the main event loop.
    fn save_message_async(&self, msg: &Message) {
        self.writer.send_message(build_message_op(msg));
    }
}
