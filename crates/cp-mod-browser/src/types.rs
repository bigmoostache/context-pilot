//! State types for the browser module: `BrowserState`, `ChromeMeta`, e-refs.

use std::collections::HashMap;

use cp_base::state::runtime::State;
use cp_mod_console::manager::SessionHandle;
use serde::{Deserialize, Serialize};

/// Console-server key-prefix namespace for browser-owned sessions.
///
/// Must not collide with the console module's `c_*` namespace — orphan
/// cleanup is scoped per prefix (see `cp_mod_console::CONSOLE_KEY_PREFIX`).
pub const BROWSER_KEY_PREFIX: &str = "browser_";

/// Serializable metadata persisted across TUI reloads so the module can
/// reconnect to a still-running Chrome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChromeMeta {
    /// Console-server session key (e.g. `browser_1`).
    pub session_key: String,
    /// OS process ID of Chrome.
    pub pid: u32,
    /// Shell command used to launch Chrome.
    pub command: String,
    /// Absolute path to the session log file (captures Chrome stderr).
    pub log_path: String,
    /// Timestamp (ms since epoch) when Chrome was spawned.
    pub started_at: u64,
    /// Browser-level `DevTools` WebSocket URL (stable for the process lifetime).
    pub ws_url: String,
    /// Whether Chrome runs headless.
    pub headless: bool,
}

/// One interactive element captured by `browser_snapshot`.
#[derive(Debug, Clone)]
pub struct Eref {
    /// Short stable reference id (e.g. `e12`).
    pub id: String,
    /// CSS selector uniquely addressing the element.
    pub selector: String,
    /// Element role summary (e.g. `button`, `input:text`, `link`).
    pub role: String,
    /// Visible label / accessible name (truncated).
    pub label: String,
}

/// Module-owned runtime state, stored in `State.module_data`.
#[derive(Debug, Default)]
pub struct BrowserState {
    /// Metadata for the managed Chrome process (None = no browser running).
    pub meta: Option<ChromeMeta>,
    /// Console-server handle for the Chrome process (status / kill).
    pub handle: Option<SessionHandle>,
    /// Live CDP connection — rebuilt lazily on first use, never serialized.
    pub client: Option<crate::client::Client>,
    /// e-ref table from the latest snapshot.
    pub erefs: Vec<Eref>,
    /// Quick lookup: e-ref id → CSS selector.
    pub eref_selectors: HashMap<String, String>,
    /// Full snapshot text shown in the Browser panel (paginated).
    pub snapshot_text: String,
    /// Human-readable description of the last action + outcome.
    pub last_action: String,
    /// URL of the current page (digest line in the panel).
    pub current_url: String,
    /// Title of the current page (digest line in the panel).
    pub current_title: String,
    /// Monotonic counter for generating unique session keys.
    pub next_session_id: usize,
}

impl BrowserState {
    /// Create an empty browser state with the session counter at 1.
    #[must_use]
    pub fn new() -> Self {
        Self { next_session_id: 1, ..Self::default() }
    }

    /// Get shared ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if the browser module state was never initialized.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Get mutable ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if the browser module state was never initialized.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }

    /// Replace the e-ref table from a fresh snapshot.
    pub fn set_erefs(&mut self, erefs: Vec<Eref>) {
        self.eref_selectors = erefs.iter().map(|e| (e.id.clone(), e.selector.clone())).collect();
        self.erefs = erefs;
    }

    /// Resolve a tool `ref`/`selector` pair to a concrete CSS selector.
    /// e-refs (e.g. `e12`) take precedence; raw selectors pass through.
    #[must_use]
    pub fn resolve_selector(&self, eref: Option<&str>, selector: Option<&str>) -> Option<String> {
        eref.map_or_else(
            || selector.map(ToString::to_string),
            |r| self.eref_selectors.get(r).cloned(),
        )
    }
}
