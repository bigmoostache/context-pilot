//! Persistent watcher: fires a notification when idle + `MY_TURN` thread exists.
//!
//! Replaces the old hand-rolled `check_my_turn_threads` + `notified_my_turn_id`
//! debounce mechanism with a standard `Watcher` impl on the spine's universal
//! notification bus — the same path every other watcher (coucou, console,
//! callback, OCR) already uses.
//!
//! **Flood caveat (deferred).** `is_persistent` keeps the watcher alive after
//! firing, and `check()` returns `Some` on every poll cycle (~50 ms) while the
//! condition holds. A dedicated anti-flood guard is planned but deliberately
//! deferred per user decision — it will be added separately.

use cp_base::panels::now_ms;
use cp_base::state::runtime::State;
use cp_base::state::watchers::Watcher;
use cp_base::state::watchers::carriers::WatcherResult;

use crate::types::{ThreadStatus, ThreadsState};

/// Watcher id — module-level const so `fn id()` returns a reference rather than
/// a literal (dodges `unnecessary_literal_bound` without a redundant annotation).
const WATCHER_ID: &str = "my_turn_watcher";
/// Watcher description (same trick as [`WATCHER_ID`]).
const WATCHER_DESC: &str = "Detects idle agent with MY_TURN threads needing a response";

/// Detects an idle agent with unattended `MY_TURN` threads.
#[derive(Debug, Clone, Copy)]
pub struct IdleMyTurnDetector {
    /// Epoch-ms when this watcher was registered.
    registered: u64,
}

impl Default for IdleMyTurnDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl IdleMyTurnDetector {
    /// Create a new detector, stamped now.
    #[must_use]
    pub fn new() -> Self {
        Self { registered: now_ms() }
    }
}

impl Watcher for IdleMyTurnDetector {
    fn id(&self) -> &str {
        WATCHER_ID
    }

    fn description(&self) -> &str {
        WATCHER_DESC
    }

    fn is_blocking(&self) -> bool {
        false // async → spine notification
    }

    fn tool_use_id(&self) -> Option<&str> {
        None
    }

    fn check(&self, state: &State) -> Option<WatcherResult> {
        // Only fire when the agent is NOT streaming (i.e. idle).
        if state.flags.stream.phase.is_streaming() {
            return None;
        }

        let ts = ThreadsState::get(state);
        let thread = ts.threads.iter().find(|t| !t.archived && !t.paused && t.status == ThreadStatus::MyTurn)?;

        Some(WatcherResult {
            description: format!(
                "Thread \"{}\" ({}) is MY_TURN and needs a response. \
                 Use Read to focus on it, then Send your reply.",
                thread.name, thread.id,
            ),
            panel_id: None,
            tool_use_id: None,
            close_panel: false,
            create_panel: None,
            processed_already: false,
            kill_session: None,
            preserves_tempo: true,
            create_dyn_panel: None,
        })
    }

    fn check_timeout(&self) -> Option<WatcherResult> {
        None // never times out
    }

    fn registered_ms(&self) -> u64 {
        self.registered
    }

    fn source_tag(&self) -> &'static str {
        "threads"
    }

    fn is_persistent(&self) -> bool {
        true // survives firing — stays in the registry
    }
}
