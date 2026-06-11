//! State types for the browser module: `BrowserState`, `ChromeMeta`, e-refs.
//!
//! Threading model (off-main-thread CDP): every browser tool's slow CDP work
//! runs on a worker thread via `spawn_async_tool` so the TUI event loop never
//! freezes. The pieces the worker needs are therefore behind `Arc`:
//! - `conn`: the cached CDP `Client`, in an `Arc<Mutex<Option<…>>>` slot the
//!   worker locks to reuse-or-reconnect (so the connection persists across calls
//!   without the main thread ever touching it).
//! - `shared`: worker-written runtime data (snapshot e-refs, last action, url,
//!   title) the main-thread panel/overview read back under a short lock.
//! - `op_lock`: serializes CDP ops so two workers never interleave on the one
//!   transport.
//! Chrome *process* lifecycle (`meta`/`handle`) stays main-thread-owned — it's a
//! one-shot spawn and lives on the persistence/save-load path.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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

/// Shared, lockable slot holding the cached CDP connection.
///
/// Lives behind an `Arc<Mutex<…>>` so a worker thread can reuse-or-reconnect the
/// `Client` and persist it for the next call, all without the main thread.
pub type ConnSlot = Arc<Mutex<Option<Arc<crate::client::Client>>>>;

/// Worker-written runtime data, read back by the main-thread panel/overview.
///
/// Guarded by a `Mutex` inside `BrowserState.shared`. The worker is the sole
/// writer (ops are serialized by `op_lock`); the main thread only ever takes a
/// brief read lock to render the digest or resolve an e-ref → selector.
#[derive(Debug, Default)]
pub struct SharedBrowser {
    /// e-ref table from the latest snapshot.
    pub erefs: Vec<Eref>,
    /// Quick lookup: e-ref id → CSS selector.
    pub eref_selectors: HashMap<String, String>,
    /// Full snapshot text shown in the Browser panel (paginated).
    pub snapshot_text: String,
    /// Human-readable description of the last action + outcome.
    pub last_action: String,
    /// URL of the current page (digest line in the panel).
    pub url: String,
    /// Title of the current page (digest line in the panel).
    pub title: String,
}

impl SharedBrowser {
    /// Replace the e-ref table from a fresh snapshot.
    pub fn set_erefs(&mut self, erefs: Vec<Eref>) {
        self.eref_selectors = erefs.iter().map(|e| (e.id.clone(), e.selector.clone())).collect();
        self.erefs = erefs;
    }

    /// Resolve a tool `ref`/`selector` pair to a concrete CSS selector.
    /// e-refs (e.g. `e12`) take precedence; raw selectors pass through.
    #[must_use]
    pub fn resolve_selector(&self, eref: Option<&str>, selector: Option<&str>) -> Option<String> {
        eref.map_or_else(|| selector.map(ToString::to_string), |r| self.eref_selectors.get(r).cloned())
    }
}

/// Module-owned runtime state, stored in `State.module_data`.
#[derive(Debug, Default)]
pub struct BrowserState {
    /// Metadata for the managed Chrome process (None = no browser running).
    /// Main-thread-owned (persistence path).
    pub meta: Option<ChromeMeta>,
    /// Console-server handle for the Chrome process (status / kill).
    /// Main-thread-owned (persistence path).
    pub handle: Option<SessionHandle>,
    /// Cached CDP connection — rebuilt lazily on a worker thread, never serialized.
    pub conn: ConnSlot,
    /// Worker-written runtime data (e-refs, last action, url, title).
    pub shared: Arc<Mutex<SharedBrowser>>,
    /// Serializes CDP ops so concurrent workers never interleave on the one
    /// transport. A worker holds this for the duration of its op.
    pub op_lock: Arc<Mutex<()>>,
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

    /// Whether the managed Chrome process is alive.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.handle.as_ref().is_some_and(|h| !h.get_status().is_terminal())
    }

    /// Drop the cached CDP connection and clear worker-written runtime data.
    /// Called on `browser_close`/kill — the Chrome process is torn down
    /// separately by `lifecycle::kill_chrome`.
    ///
    /// NON-BLOCKING (P08 re-freeze fix): uses `try_lock`, never a blocking
    /// `lock()`. `browser_close` runs synchronously on the main thread; a worker
    /// can be inside `connect_shared` holding `conn` across `Client::connect` /
    /// `is_alive` (a CDP round-trip — seconds, unbounded if Chrome is hung). A
    /// blocking `conn.lock()` here would stall the main event loop for that whole
    /// window — re-introducing the exact freeze the off-main-thread refactor
    /// removed. If `conn` is contended we simply abandon the drop: this is safe
    /// because `kill_chrome` is killing the Chrome process anyway, so the worker's
    /// in-flight op fails out and releases `conn` shortly, and the now-stale
    /// `Arc<Client>` is transparently replaced on the next op's `connect_shared`
    /// (`is_alive` → false → reconnect). `shared` is only ever held briefly, but
    /// we `try_lock` it too for symmetry. (`try_lock` skips on BOTH contention and
    /// poison — both are fine to skip here.)
    pub fn clear_session(&self) {
        if let Ok(mut slot) = self.conn.try_lock() {
            *slot = None;
        }
        if let Ok(mut s) = self.shared.try_lock() {
            *s = SharedBrowser::default();
        }
    }
}
