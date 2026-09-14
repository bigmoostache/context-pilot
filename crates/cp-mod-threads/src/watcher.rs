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

use std::sync::atomic::{AtomicU64, Ordering};

use cp_base::panels::now_ms;
use cp_base::state::runtime::State;
use cp_base::state::watchers::Watcher;
use cp_base::state::watchers::carriers::WatcherResult;

use crate::types::{FocusState, ThreadStatus, ThreadsState};

/// Watcher id — module-level const so `fn id()` returns a reference rather than
/// a literal (dodges `unnecessary_literal_bound` without a redundant annotation).
const WATCHER_ID: &str = "my_turn_watcher";
/// Watcher description (same trick as [`WATCHER_ID`]).
const WATCHER_DESC: &str = "Detects idle agent with MY_TURN threads needing a response";

/// Minimum interval between watcher fires (ms). Prevents flood from the
/// persistent watcher firing every ~50ms poll cycle.
const COOLDOWN_MS: u64 = 5_000;

/// Detects an idle agent with unattended `MY_TURN` threads.
#[derive(Debug)]
pub struct IdleMyTurnDetector {
    /// Epoch-ms when this watcher was registered.
    registered: u64,
    /// Epoch-ms when the watcher last produced a notification.
    /// Uses `AtomicU64` because `Watcher: Sync` and `check()` takes `&self`.
    last_fired_ms: AtomicU64,
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
        Self { registered: now_ms(), last_fired_ms: AtomicU64::new(0) }
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

        // Cooldown: don't fire again within COOLDOWN_MS of the last fire.
        let now = now_ms();
        let last = self.last_fired_ms.load(Ordering::Relaxed);
        if last > 0 && now.saturating_sub(last) < COOLDOWN_MS {
            return None;
        }

        let ts = ThreadsState::get(state);
        let fs = FocusState::get(state);

        // Prefer the focused thread (it's the one the agent was working on when
        // it stopped). Fall back to any MY_TURN thread.
        let focused_tid = fs.focused_thread_id.as_deref();
        let thread = focused_tid
            .and_then(|fid| {
                ts.threads.iter().find(|t| t.id == fid && !t.archived && !t.paused && t.status == ThreadStatus::MyTurn)
            })
            .or_else(|| ts.threads.iter().find(|t| !t.archived && !t.paused && t.status == ThreadStatus::MyTurn))?;

        // Record the fire time for cooldown.
        self.last_fired_ms.store(now, Ordering::Relaxed);

        // Suppress on abnormal stream endings (refusal, max_tokens, content
        // filter, errors). Only fire for normal completion ("end_turn") or
        // mid-tool-use ("tool_use") — those indicate the agent voluntarily
        // stopped and should be nudged.
        if !is_normal_stop(state.last_stop_reason.as_deref()) {
            return Some(WatcherResult {
                description: format!(
                    "Thread \"{}\" ({}) is MY_TURN but the last LLM turn ended \
                     abnormally (reason: {}). Send a message in that thread \
                     explaining to the user that a guardrail or error fired, \
                     and suggest changing the model to continue.",
                    thread.name,
                    thread.id,
                    state.last_stop_reason.as_deref().unwrap_or("unknown/error"),
                ),
                panel_id: None,
                tool_use_id: None,
                close_panel: false,
                create_panel: None,
                processed_already: false,
                kill_session: None,
                preserves_tempo: true,
                create_dyn_panel: None,
            });
        }

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

/// Whether the last stream stop reason indicates a normal voluntary stop.
///
/// Returns `true` for `"end_turn"` and `"tool_use"` — the agent finished on
/// its own terms and should be nudged if it left a `MY_TURN` thread unanswered.
///
/// Returns `false` for everything else: `None` (stream error — `last_stop_reason`
/// is not set on the error path), `"max_tokens"`, `"content_filter"`, or any
/// other abnormal/unknown reason. These indicate a guardrail or infrastructure
/// issue, not agent laziness.
fn is_normal_stop(reason: Option<&str>) -> bool {
    matches!(reason, Some("end_turn" | "tool_use"))
}
