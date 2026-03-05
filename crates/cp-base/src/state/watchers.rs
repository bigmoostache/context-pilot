//! Watcher trait and registry for asynchronous condition monitoring.
//!
//! Modules register watchers (via WatcherRegistry in State) to monitor
//! conditions like process exit, pattern matching, or timers.
//! The spine module polls the registry and fires notifications when
//! conditions are met.

use crate::state::runtime::State;

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
