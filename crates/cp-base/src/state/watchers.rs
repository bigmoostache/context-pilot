//! Watcher trait and registry for asynchronous condition monitoring.
//!
//! Modules register watchers (via WatcherRegistry in State) to monitor
//! conditions like process exit, pattern matching, or timers.
//! The spine module polls the registry and fires notifications when
//! conditions are met.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError};

use crate::state::runtime::State;

/// Placeholder in [`ToolOutput::content`] that gets replaced with the actual
/// panel ID after [`DynPanel`] creation.  Used by async tools that create panels.
pub const DYN_PANEL_ID_PLACEHOLDER: &str = "__PANEL_ID__";

/// Prefix prepended to [`WatcherResult::description`] when the async tool failed.
///
/// `struct_excessive_bools` is `forbid`-level, so we can't add an `is_error` bool
/// to [`WatcherResult`].  Instead, the async worker thread encodes error status
/// in the description string, and `cleanup.rs` strips the prefix during sentinel
/// replacement while setting `ToolResult::is_error = true`.
pub const ASYNC_ERROR_PREFIX: &str = "__ASYNC_ERR__";

/// Result of a satisfied watcher condition.
#[derive(Debug)]
pub struct WatcherResult {
    /// Human-readable description of what happened.
    pub description: String,
    /// Panel ID associated with this watcher (if any).
    pub panel_id: Option<String>,
    /// Tool use ID for blocking watchers that need sentinel replacement.
    pub tool_use_id: Option<String>,
    /// If true, the panel should be auto-closed (removed from context).
    /// Used by callback watchers to clean up console panels on success.
    pub close_panel: bool,
    /// If set, `tool_cleanup` should create a console panel for this session.
    /// Used by callback watchers that defer panel creation until failure.
    /// Contains (`session_key`, `display_name`, command, description, cwd).
    pub create_panel: Option<DeferredPanel>,
    /// If true, the spine notification is created already processed (no auto-continuation).
    /// Used for success notifications that don't need attention.
    pub processed_already: bool,
    /// If set, kill and remove this console session after processing.
    /// Used by `easy_bash` inline path to clean up sessions that have no panel.
    pub kill_session: Option<String>,
    /// When `true`, the cleanup code does NOT break tempo for this watcher result.
    /// Used by blocking watchers whose resolution did not create or modify any panel
    /// (e.g., `easy_bash` inline path with short output).
    pub preserves_tempo: bool,
    /// If set, create a generic dynamic panel when this watcher fires.
    /// Unlike `create_panel` (console-specific), this works for any panel type.
    pub create_dyn_panel: Option<DynPanel>,
}

/// Info needed to create a console panel after a watcher fires.
#[derive(Debug)]
pub struct DeferredPanel {
    /// Console session key for reconnection.
    pub session_key: String,
    /// Human-readable name for the panel tab.
    pub display_name: String,
    /// Shell command that was executed.
    pub command: String,
    /// Short description for the panel header.
    pub description: String,
    /// Working directory (None = project root).
    pub cwd: Option<String>,
    /// ID of the callback that created this panel.
    pub callback_id: String,
    /// Display name of the callback.
    pub callback_name: String,
}

/// Info needed to create a generic dynamic panel when a watcher fires.
///
/// Unlike [`DeferredPanel`] (console-specific), this works for any panel type
/// (brave results, firecrawl results, search results, etc.).
/// Used by async tool execution to create panels after HTTP/subprocess completion.
#[derive(Debug)]
pub struct DynPanel {
    /// Context type string (e.g., `"brave_result"`, `"firecrawl_result"`).
    pub context_type: String,
    /// Human-readable panel title.
    pub display_name: String,
    /// Key-value metadata to set via `Entry::set_meta`.
    pub metadata: Vec<(String, String)>,
    /// Panel content to set as `cached_content` immediately.
    /// When set, the panel displays content without waiting for a cache restore cycle.
    pub content: Option<String>,
}

/// A watcher monitors a condition and reports when it's satisfied.
///
/// Watchers are polled periodically by the app event loop.
/// When `check()` returns `Some(WatcherResult)`, the watcher is removed
/// and either:
/// - Blocking: the sentinel tool result is replaced with the real result
/// - Async: a spine notification is created
pub trait Watcher: Send + Sync {
    /// Unique identifier for this watcher instance (e.g., "`console_c_42_exit`").
    fn id(&self) -> &str;

    /// Human-readable description shown in the Spine panel (e.g., "Waiting for cargo build to exit").
    fn description(&self) -> &str;

    /// Whether this watcher blocks tool execution (sentinel replacement)
    /// or is async (spine notification).
    fn is_blocking(&self) -> bool;

    /// Tool use ID for blocking watchers. Used to replace the sentinel
    /// in pending tool results.
    fn tool_use_id(&self) -> Option<&str>;

    /// Check if the condition is met. Returns Some(result) when satisfied.
    /// Called every poll cycle (~50ms). Must be non-blocking.
    ///
    /// The `state` reference is read-only. Watchers should read from
    /// `module_data` (e.g., `ConsoleState` session buffers) to check conditions.
    fn check(&self, state: &State) -> Option<WatcherResult>;

    /// Check if this watcher has timed out. Returns Some(result) with
    /// a timeout message if deadline has passed.
    fn check_timeout(&self) -> Option<WatcherResult>;

    /// Timestamp (ms since epoch) when this watcher was registered.
    fn registered_ms(&self) -> u64;

    /// Source tag for categorizing notifications (e.g., "console").
    fn source_tag(&self) -> &'static str;

    /// Whether this watcher should be silently removed. Called every poll
    /// cycle. Return `true` if the watched resource no longer exists
    /// (e.g., console session gone after reload). Default: `false`.
    fn suicide(&self, _state: &State) -> bool {
        false
    }

    /// Whether this watcher was created by `easy_bash` (needs special result formatting).
    fn is_easy_bash(&self) -> bool {
        false
    }

    /// Whether this watcher survives after firing. Default: false (one-shot).
    /// Persistent watchers stay in the registry after `check()` returns Some,
    /// and can fire again on subsequent polls. Use for recurring conditions
    /// like "todos still incomplete".
    fn is_persistent(&self) -> bool {
        false
    }

    /// Target fire time in ms since epoch (for time-based watchers like coucou).
    /// Returns None for condition-based watchers (console exit/pattern).
    /// Used for persistence across reloads.
    fn fire_at_ms(&self) -> Option<u64> {
        None
    }

    /// Human-readable message payload (for watchers that carry a message).
    /// Used for persistence across reloads.
    fn message(&self) -> Option<&str> {
        None
    }
}

/// Registry holding active watchers. Stored in State via `TypeMap`.
/// Initialized by the spine module, accessed by any module that
/// registers watchers.
pub struct WatcherRegistry {
    /// Active watchers, polled each tick by the event loop.
    pub watchers: Vec<Box<dyn Watcher>>,
}

impl std::fmt::Debug for WatcherRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatcherRegistry").field("watchers_count", &self.watchers.len()).finish()
    }
}

impl Default for WatcherRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl WatcherRegistry {
    /// Create an empty watcher registry.
    #[must_use]
    pub fn new() -> Self {
        Self { watchers: Vec::new() }
    }

    /// Register a new watcher.
    pub fn register(&mut self, watcher: Box<dyn Watcher>) {
        self.watchers.push(watcher);
    }

    /// Poll all watchers and return satisfied results.
    /// One-shot watchers are removed when they fire.
    /// Persistent watchers stay in the registry and can fire again.
    /// Returns (`blocking_results`, `async_results`).
    pub fn poll_all(&mut self, state: &State) -> (Vec<WatcherResult>, Vec<WatcherResult>) {
        let mut blocking = Vec::new();
        let mut async_results = Vec::new();
        let mut remaining = Vec::new();

        for watcher in self.watchers.drain(..) {
            // Suicide: silently remove watchers whose resource no longer exists
            if watcher.suicide(state) {
                continue;
            }

            // Check condition first (before timeout) to avoid race
            if let Some(result) = watcher.check(state) {
                if watcher.is_blocking() {
                    blocking.push(result);
                } else {
                    async_results.push(result);
                }
                // Persistent watchers survive after firing
                if watcher.is_persistent() {
                    remaining.push(watcher);
                }
                continue;
            }

            // Then check timeout
            if let Some(result) = watcher.check_timeout() {
                if watcher.is_blocking() {
                    blocking.push(result);
                } else {
                    async_results.push(result);
                }
                continue;
            }

            remaining.push(watcher);
        }

        self.watchers = remaining;
        (blocking, async_results)
    }

    /// Get a read-only view of active watchers (for rendering in Spine panel).
    #[must_use]
    pub fn active_watchers(&self) -> &[Box<dyn Watcher>] {
        &self.watchers
    }

    /// Check if any blocking watchers are active.
    #[must_use]
    pub fn has_blocking_watchers(&self) -> bool {
        self.watchers.iter().any(|w| w.is_blocking())
    }

    /// Check if a watcher with the given source tag exists.
    #[must_use]
    pub fn has_watcher_with_tag(&self, tag: &str) -> bool {
        self.watchers.iter().any(|w| w.source_tag() == tag)
    }

    /// Remove all watchers with the given source tag.
    pub fn remove_by_tag(&mut self, tag: &str) {
        self.watchers.retain(|w| w.source_tag() != tag);
    }

    /// Get from State via `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Get mutable from State via `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }
}

// ─── ChannelWatcher ─────────────────────────────────────────────────────────

/// A watcher that polls an `mpsc::Receiver` for a result from a background thread.
///
/// Used by [`spawn_async_tool`](crate::tools::spawn_async_tool) to make tool
/// execution non-blocking. The worker thread sends a [`WatcherResult`] when done;
/// the main event loop picks it up via the existing watcher polling infrastructure.
pub struct ChannelWatcher {
    /// Unique ID for this watcher instance.
    id: String,
    /// Human-readable description for the Spine panel.
    desc: String,
    /// Tool use ID for sentinel replacement in blocking path.
    tuid: String,
    /// Receiver end of the channel from the worker thread.
    /// Wrapped in `Mutex` for `Sync` (required by `dyn Watcher: Send + Sync`).
    rx: Mutex<Receiver<WatcherResult>>,
    /// Timestamp when this watcher was registered.
    registered_at_ms: u64,
    /// Absolute deadline in ms. Returns a timeout error after this point.
    deadline_ms: u64,
    /// Cooperative-cancellation flag flipped `true` when the deadline passes.
    ///
    /// The worker thread holds a clone and checks it before mutating any shared
    /// state, so a timed-out (abandoned) worker that keeps running can no longer
    /// clobber state belonging to a newer op (the browser P11/P03 zombie-write
    /// race). `None` for watchers whose tools don't opt into cancellation.
    cancel_on_timeout: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl std::fmt::Debug for ChannelWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelWatcher")
            .field("id", &self.id)
            .field("desc", &self.desc)
            .field("tuid", &self.tuid)
            .field("registered_at_ms", &self.registered_at_ms)
            .field("deadline_ms", &self.deadline_ms)
            .finish_non_exhaustive()
    }
}

/// Construction parameters for [`ChannelWatcher::new`].
///
/// Bundled into a struct so the constructor stays within the argument-count
/// lint budget while still threading the cooperative-cancellation flag.
pub struct ChannelWatcherInit {
    /// Shown in the Spine panel watchers list.
    pub description: String,
    /// Matches the sentinel `ToolResult` for replacement; derives the watcher ID.
    pub tool_use_id: String,
    /// Receiving end of the channel from the worker thread.
    pub rx: Receiver<WatcherResult>,
    /// How long to wait before returning a timeout error.
    pub timeout_ms: u64,
    /// Optional flag set `true` on timeout so a still-running (abandoned) worker
    /// can cooperatively skip its stale writes.
    pub cancel_on_timeout: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl std::fmt::Debug for ChannelWatcherInit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelWatcherInit")
            .field("description", &self.description)
            .field("tool_use_id", &self.tool_use_id)
            .field("timeout_ms", &self.timeout_ms)
            .finish_non_exhaustive()
    }
}

impl ChannelWatcher {
    /// Create a new channel watcher from its [`ChannelWatcherInit`] parameters.
    #[must_use]
    pub fn new(init: ChannelWatcherInit) -> Self {
        let now = crate::panels::now_ms();
        Self {
            id: format!("async_tool_{}", init.tool_use_id),
            desc: init.description,
            tuid: init.tool_use_id,
            rx: Mutex::new(init.rx),
            registered_at_ms: now,
            deadline_ms: now.saturating_add(init.timeout_ms),
            cancel_on_timeout: init.cancel_on_timeout,
        }
    }
}

impl Watcher for ChannelWatcher {
    fn id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        &self.desc
    }

    fn is_blocking(&self) -> bool {
        true
    }

    fn tool_use_id(&self) -> Option<&str> {
        Some(&self.tuid)
    }

    fn check(&self, _state: &State) -> Option<WatcherResult> {
        let Ok(rx) = self.rx.lock() else {
            return Some(WatcherResult {
                description: "Async tool watcher failed (lock poisoned)".to_string(),
                panel_id: None,
                tool_use_id: Some(self.tuid.clone()),
                close_panel: false,
                create_panel: None,
                create_dyn_panel: None,
                processed_already: false,
                kill_session: None,
                preserves_tempo: false,
            });
        };
        match rx.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Disconnected) => Some(WatcherResult {
                description: "Async tool execution failed (worker thread panicked or dropped)".to_string(),
                panel_id: None,
                tool_use_id: Some(self.tuid.clone()),
                close_panel: false,
                create_panel: None,
                create_dyn_panel: None,
                processed_already: false,
                kill_session: None,
                preserves_tempo: false,
            }),
            Err(TryRecvError::Empty) => None,
        }
    }

    fn check_timeout(&self) -> Option<WatcherResult> {
        (crate::panels::now_ms() >= self.deadline_ms).then(|| {
            // Signal the (still-running) worker to abandon its stale writes.
            if let Some(flag) = &self.cancel_on_timeout {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            let elapsed_secs =
                crate::panels::time_arith::ms_to_secs(self.deadline_ms.saturating_sub(self.registered_at_ms));
            WatcherResult {
                description: format!("Async tool timed out after {elapsed_secs}s"),
                panel_id: None,
                tool_use_id: Some(self.tuid.clone()),
                close_panel: false,
                create_panel: None,
                create_dyn_panel: None,
                processed_already: false,
                kill_session: None,
                preserves_tempo: false,
            }
        })
    }

    fn registered_ms(&self) -> u64 {
        self.registered_at_ms
    }

    fn source_tag(&self) -> &'static str {
        "async_tool"
    }

    fn suicide(&self, _state: &State) -> bool {
        false
    }

    fn is_easy_bash(&self) -> bool {
        false
    }

    fn is_persistent(&self) -> bool {
        false
    }

    fn fire_at_ms(&self) -> Option<u64> {
        None
    }

    fn message(&self) -> Option<&str> {
        None
    }
}
