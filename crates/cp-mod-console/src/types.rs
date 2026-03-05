use std::collections::HashMap;

use cp_base::panels::now_ms;
use cp_base::state::runtime::State;
use cp_base::state::watchers::{Watcher, WatcherResult};
use serde::{Deserialize, Serialize};

use crate::manager::SessionHandle;

/// Serializable metadata for a console session (used for persistence across reloads).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    /// OS process ID.
    pub pid: u32,
    /// Shell command that was executed.
    pub command: String,
    /// Working directory (None = project root).
    pub cwd: Option<String>,
    /// Absolute path to the log file capturing stdout/stderr.
    pub log_path: String,
    /// Timestamp (ms since epoch) when the session was spawned.
    pub started_at: u64,
}

/// Process lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    /// Process is actively running.
    Running,
    /// Process exited normally with the given code.
    Finished(i32),
    /// Process exited with a non-zero (failure) code.
    Failed(i32),
    /// Process was forcefully killed (SIGKILL).
    Killed,
}

impl ProcessStatus {
    /// Human-readable label (e.g., "running", "exited(0)", "failed(1)").
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Running => "running".to_string(),
            Self::Finished(code) => format!("exited({code})"),
            Self::Failed(code) => format!("failed({code})"),
            Self::Killed => "killed".to_string(),
        }
    }

    /// Whether the process has reached a terminal state (not running).
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }

    /// Exit code if terminal (Killed → -9), None if still running.
    #[must_use]
    pub const fn exit_code(self) -> Option<i32> {
        match self {
            Self::Finished(c) | Self::Failed(c) => Some(c),
            Self::Running => None,
            Self::Killed => Some(-9),
        }
    }
}

/// Module-owned state for the Console module.
/// Stored in `State.module_data` via `TypeMap`.
#[derive(Debug)]
pub struct ConsoleState {
    /// Active session handles, keyed by session name (e.g., "`c_42`").
    pub sessions: HashMap<String, SessionHandle>,
    /// Monotonic counter for generating unique session keys.
    pub next_session_id: usize,
}

impl Default for ConsoleState {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleState {
    /// Create an empty console state with session counter at 1.
    #[must_use]
    pub fn new() -> Self {
        Self { sessions: HashMap::new(), next_session_id: 1 }
    }

    /// Get shared ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Get mutable ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }

    /// Kill a session by name and update its panel metadata.
    pub fn kill_session(state: &mut State, name: &str) {
        let cs = Self::get_mut(state);
        if let Some(handle) = cs.sessions.get(name) {
            handle.kill();
        }
    }

    /// Shutdown all sessions (called during reset).
    pub fn shutdown_all(state: &mut State) {
        let cs = Self::get_mut(state);
        let mut keys: Vec<_> = cs.sessions.keys().cloned().collect();
        keys.sort();
        for key in keys {
            if let Some(handle) = cs.sessions.remove(&key) {
                handle.kill();
            }
        }
    }
}

/// Format a wait result message for the LLM.
#[must_use]
pub fn format_wait_result(name: &str, exit_code: Option<i32>, panel_id: &str, last_lines: &str) -> String {
    let code_str = exit_code.map_or_else(|| "?".to_string(), |c| c.to_string());
    let now = now_ms();
    format!(
        "Console '{name}' condition met (exit_code={code_str}, panel={panel_id}, time={now}ms)\nLast output:\n{last_lines}"
    )
}

// ============================================================
// Console Watcher — implements cp_base::state::watchers::Watcher trait
// ============================================================

/// A watcher that monitors a console session for a condition.
#[derive(Debug)]
pub struct ConsoleWatcher {
    /// Unique ID for this watcher (e.g., "`console_c_42_exit`").
    pub watcher_id: String,
    /// Session key in `ConsoleState` (e.g., "`c_42`").
    pub session_name: String,
    /// Watch mode: "exit" or "pattern".
    pub mode: String,
    /// Regex pattern to match (when mode="pattern").
    pub pattern: Option<String>,
    /// Whether this watcher blocks tool execution.
    pub blocking: bool,
    /// Tool use ID for sentinel replacement (blocking watchers).
    pub tool_use_id: Option<String>,
    /// When this watcher was registered (ms since epoch).
    pub registered_at_ms: u64,
    /// Deadline for timeout (ms since epoch). None = no timeout.
    pub deadline_ms: Option<u64>,
    /// If true, format result as `easy_bash` output summary.
    pub easy_bash: bool,
    /// Panel ID for this console session.
    pub panel_id: String,
    /// Human-readable description.
    pub desc: String,
}

impl Watcher for ConsoleWatcher {
    fn id(&self) -> &str {
        &self.watcher_id
    }

    fn description(&self) -> &str {
        &self.desc
    }

    fn is_blocking(&self) -> bool {
        self.blocking
    }

    fn tool_use_id(&self) -> Option<&str> {
        self.tool_use_id.as_deref()
    }

    fn check(&self, state: &State) -> Option<WatcherResult> {
        let cs = ConsoleState::get(state);
        let handle = cs.sessions.get(&self.session_name)?;

        let satisfied = match self.mode.as_str() {
            "exit" => handle.get_status().is_terminal(),
            "pattern" => self.pattern.as_ref().is_some_and(|pat| handle.buffer.contains_pattern(pat)),
            _ => false,
        };

        if !satisfied {
            return None;
        }

        if self.easy_bash {
            let output =
                std::fs::read_to_string(cs.sessions.get(&self.session_name).map_or("", |h| h.log_path.as_str()))
                    .unwrap_or_default();
            let exit_code = handle.get_status().exit_code().unwrap_or(-1);
            let line_count = output.lines().count();
            Some(WatcherResult {
                description: format!("Output in {} ({} lines, exit_code={})", self.panel_id, line_count, exit_code),
                panel_id: Some(self.panel_id.clone()),
                tool_use_id: self.tool_use_id.clone(),
                close_panel: false,
                create_panel: None,
                processed_already: false,
            })
        } else {
            let exit_code = handle.get_status().exit_code();
            let last_lines = handle.buffer.last_n_lines(5);
            Some(WatcherResult {
                description: format_wait_result(&self.session_name, exit_code, &self.panel_id, &last_lines),
                panel_id: Some(self.panel_id.clone()),
                tool_use_id: self.tool_use_id.clone(),
                close_panel: false,
                create_panel: None,
                processed_already: false,
            })
        }
    }

    fn check_timeout(&self) -> Option<WatcherResult> {
        let deadline = self.deadline_ms?;
        let now = now_ms();
        if now < deadline {
            return None;
        }

        let elapsed_s = cp_base::panels::time_arith::ms_to_secs(now.saturating_sub(self.registered_at_ms));

        if self.easy_bash {
            Some(WatcherResult {
                description: format!(
                    "Output in {} (TIMED OUT after {}s, process may still be running)",
                    self.panel_id, elapsed_s
                ),
                panel_id: Some(self.panel_id.clone()),
                tool_use_id: self.tool_use_id.clone(),
                close_panel: false,
                create_panel: None,
                processed_already: false,
            })
        } else {
            Some(WatcherResult {
                description: format!(
                    "Console '{}' wait TIMED OUT after {}s (panel={})",
                    self.session_name, elapsed_s, self.panel_id
                ),
                panel_id: Some(self.panel_id.clone()),
                tool_use_id: self.tool_use_id.clone(),
                close_panel: false,
                create_panel: None,
                processed_already: false,
            })
        }
    }

    fn registered_ms(&self) -> u64 {
        self.registered_at_ms
    }

    fn source_tag(&self) -> &'static str {
        "console"
    }

    fn suicide(&self, state: &State) -> bool {
        let cs = ConsoleState::get(state);
        !cs.sessions.contains_key(&self.session_name)
    }

    fn is_easy_bash(&self) -> bool {
        self.easy_bash
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
