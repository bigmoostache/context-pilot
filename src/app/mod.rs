/// Application actions and command dispatch.
pub(crate) mod actions;
/// Context management: preparation, detachment, defaults.
mod context;
/// Keyboard/mouse event handling and routing.
pub(crate) mod events;
/// Panel trait bridge: rendering, context collection, registry lookup.
pub(crate) mod panels;
/// Centralized prompt assembly for all LLM providers.
pub(crate) mod prompt;
/// Reverie sub-agent: trigger, tools, and lifecycle.
pub(crate) mod reverie;
/// Main event loop, streaming, tool pipeline, watchers.
pub(crate) mod run;

pub(crate) use context::{ensure_default_agent, ensure_default_contexts};

use std::sync::mpsc::{Receiver, Sender};

use crate::infra::tools::ToolUse;
use crate::infra::watcher::FileWatcher;
use crate::state::State;
use crate::state::cache::CacheUpdate;
use crate::state::persistence::PersistenceWriter;
use crate::ui::help::CommandPalette;

/// Deferred `StreamDone` data: (`input_tokens`, `output_tokens`, `cache_hit`, `cache_miss`, `stop_reason`, `bp_hashes`, `bp_panel_ids`, `alive_count`, `alive_positions_permille`).
pub(crate) type PendingDone = (usize, usize, usize, usize, Option<String>, Vec<String>, Vec<String>, usize, Vec<u16>);

/// Per-thread main-stream state — holds the receiver channel for a running
/// thread stream (the thread-centric analogue of [`ReverieStream`]).
///
/// One entry lives in [`App::thread_streams`] per actively-streaming thread.
/// Created at stream start (one of the three start sites: `check_spine`,
/// `continue_streaming`, `handle_retry`), drained by `process_stream_events`.
///
/// At N=1 there is a single resident thread, so the map holds a single entry
/// keyed by [`App::main_stream_key`]. Phase C3 generalises the key to the
/// actually-advancing thread id so the loop can drive several threads at once.
pub(crate) struct ThreadStream {
    /// Receiver for this thread's LLM stream events.
    pub rx: Receiver<crate::infra::api::StreamEvent>,
}

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
    /// Sender for cache update requests to the background cache thread.
    pub cache_tx: Sender<CacheUpdate>,
    /// Optional file-system watcher for auto-refresh on file changes.
    pub file_watcher: Option<FileWatcher>,
    /// Tracks which file paths are being watched
    pub watched_file_paths: std::collections::HashSet<String>,
    /// Tracks which directory paths are being watched
    pub watched_dir_paths: std::collections::HashSet<String>,
    /// Fingerprint of every module's `watch_paths()` at the last watcher sync;
    /// an unchanged fingerprint skips the sync (no watch/unwatch syscalls).
    pub watch_specs_hash: u64,
    /// Last time we checked timer-based caches
    pub last_timer_check_ms: u64,
    /// Last time we checked ownership
    pub last_ownership_check_ms: u64,
    /// Last render time for throttling
    pub last_render_ms: u64,
    /// Last forced full repaint (terminal clear + redraw of every cell)
    pub last_full_redraw_ms: u64,

    /// Last spinner animation update time
    pub last_spinner_ms: u64,
    /// Last bridge-recovery retry time — throttles the periodic
    /// `cp_mod_bridge::try_recover` attempt that self-heals a bridge whose boot
    /// lost the `flock` race on a fast relaunch (no-op once the bridge is live).
    pub last_bridge_recover_ms: u64,
    /// Last Matrix sync drain time — for periodic idle-time event polling
    pub last_chat_drain_ms: u64,
    /// Channel for API check results
    pub api_check_rx: Option<Receiver<crate::llms::ApiCheckResult>>,
    /// Whether to auto-start streaming on first loop iteration
    pub resume_stream: bool,
    /// Command palette state
    pub command_palette: CommandPalette,
    /// Background persistence writer — offloads file I/O to a dedicated thread
    pub writer: PersistenceWriter,
    /// Last poll time per panel ID — tracks when we last submitted a cache request
    /// for timer-based panels (Tmux, Git, `GitResult`, `GithubResult`, Glob, Grep).
    /// Separate from `Entry.last_refresh_ms` which tracks actual content changes.
    pub last_poll_ms: std::collections::HashMap<String, u64>,
    /// Active reverie streams keyed by `agent_id` (one per agent type)
    pub reverie_streams: std::collections::HashMap<String, ReverieStream>,
    /// Active main-thread streams keyed by thread id (one per streaming thread).
    /// At N=1 holds a single entry under [`App::main_stream_key`]; the loop
    /// (Phase C3) will key this by the advancing thread id for true concurrency.
    pub thread_streams: std::collections::HashMap<String, ThreadStream>,
    /// Fleet registry: the source of truth for every non-resident thread's
    /// runtime bundle + its execution state and role (Phase C).
    ///
    /// The resident (focused) thread's bundle lives *flat* in [`App::state`];
    /// every other thread parks its [`ThreadRuntime`](cp_base::state::runtime::bundle::ThreadRuntime)
    /// here inside an `Entry`, and the loop swaps it into `state` for one
    /// advancement step (see `advance_background_threads`). Empty at N=1 — the
    /// single resident thread is the only one that exists — so the background
    /// advancement pass is a no-op and behaviour is identical to single-thread.
    /// Population (reconcile from `ThreadsState`) is wired in C4.
    pub fleet: cp_fleet::FleetRegistry,
    /// Id of the background thread currently swapped into [`state`](Self::state)
    /// for an advancement step, or `None` when the resident is the focused
    /// thread (the normal case). It is the override half of
    /// [`resident_key`](Self::resident_key): stream spawn and drain both key by
    /// the resident thread, so during a background step they target that
    /// thread's channel rather than the focused thread's.
    pub stepping_thread: Option<String>,
    /// Result of the previous iteration's idle `event::poll(timeout)`, consumed
    /// by the next input phase so it can skip its own `poll(ZERO)`. `None` on
    /// the first iteration and after a `Restart` (which skips the idle poll).
    pub input_ready: Option<bool>,
}

// App impl block is in run/input.rs (primary), with additional methods spread
// across the run/ submodule files.
