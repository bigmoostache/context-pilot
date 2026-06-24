//! [`Backend`] shared state — the thread-safe store that transport handlers
//! read and the runtime loop writes.
//!
//! Extracted from `transport/mod.rs` to keep both files within the line budget.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use crate::inspect::StateReader;
use crate::services::auth::store::AuthStore;
use crate::services::{AvatarStore, CostBreaker, MaterializedView, NameOverrides, RetiredStore, StreamHub};
use crate::supervisor::AgentSupervisor;

use super::super::ticket::TicketStore;

/// Default per-agent SSE subscriber buffer capacity.
const DEFAULT_SUB_CAPACITY: usize = 256;

/// Shared backend state read by transport handlers and written by the runtime
/// loop.
///
/// Wrapped in an [`Arc<Mutex<Backend>>`](std::sync::Mutex) for the
/// thread-per-connection server. Handlers hold the lock only briefly and never
/// across blocking agent I/O.
#[derive(Debug)]
pub struct Backend {
    /// Per-agent projected fleet state.
    pub(crate) view: MaterializedView,
    /// Durable per-agent spend breaker.
    pub(crate) breaker: CostBreaker,
    /// Per-agent ephemeral stream fan-out.
    pub(crate) hub: StreamHub,
    /// Single-use SSE upgrade tickets.
    pub(crate) tickets: TicketStore,
    /// Read-only, mtime-cached reader of agent persistence files.
    pub(crate) inspect: StateReader,
    /// Directory of agent registry records (`<id>.json`).
    pub(crate) agents_dir: PathBuf,
    /// Agents whose tier-② state has changed since the last SSE sweep.
    /// SSE producers drain this per-agent to emit `invalidate` events.
    pub(crate) dirty_agents: HashSet<String>,
    /// Process-lifecycle manager — spawns dashboard-created agents (PTY) under
    /// a binary allow-list (R2-15).
    pub(crate) supervisor: AgentSupervisor,
    /// Root directory new agents' realm folders are created under.
    pub(crate) agents_root: PathBuf,
    /// The `cp` TUI binary the supervisor spawns (also the sole allow-list
    /// entry).
    pub(crate) agent_binary: PathBuf,
    /// Orchestrator-owned set of retired agents (T271) — stopped-but-kept
    /// agents, persisted independently of the agent-written registry records.
    pub(crate) retired: RetiredStore,
    /// Custom display-name overrides set via the dashboard (T328).
    pub(crate) names: NameOverrides,
    /// Agent profile picture store (T338).
    pub(crate) avatars: AvatarStore,
    /// Auth store — `None` when auth is disabled (`CP_AUTH_ENABLED=false`).
    /// Contains the SQLite-backed user/session/ACL database (design doc §5).
    pub(crate) auth: Option<AuthStore>,
    /// Session lifetime for newly created sessions (FR-15).
    pub(crate) session_ttl: Duration,
    /// Runtime overrides for environment variables (T404).
    ///
    /// `PUT /api/env-keys/{name}` writes here for immediate visibility; the
    /// value is also persisted to `~/.context-pilot/.env` so agents pick it up
    /// on their next launch.  `env_key_reveal` / `env_keys_list` read this
    /// map first, falling back to [`std::env::var`].
    pub(crate) env_overrides: HashMap<String, String>,
}

impl Backend {
    /// Build a backend with empty services and the given per-agent cost budget.
    ///
    /// `agents_root` is where dashboard-created agents' folders are made, and
    /// `agent_binary` is the `cp` TUI binary the supervisor may spawn — it
    /// seeds the supervisor's allow-list (R2-15), so it is the only binary that
    /// can ever be launched.
    #[must_use]
    pub fn new(
        agents_dir: PathBuf,
        budget_usd: f64,
        agents_root: PathBuf,
        agent_binary: PathBuf,
        auth: Option<AuthStore>,
        session_ttl: Duration,
    ) -> Self {
        Self {
            view: MaterializedView::new(),
            breaker: CostBreaker::new(budget_usd),
            hub: StreamHub::new(DEFAULT_SUB_CAPACITY),
            tickets: TicketStore::new(),
            inspect: StateReader::new(),
            retired: RetiredStore::load(&agents_dir),
            names: NameOverrides::load(&agents_dir),
            avatars: AvatarStore::load(&agents_dir),
            agents_dir,
            dirty_agents: HashSet::new(),
            supervisor: AgentSupervisor::new(&[agent_binary.clone()]),
            agents_root,
            agent_binary,
            auth,
            session_ttl,
            env_overrides: HashMap::new(),
        }
    }

    /// Mutable access to the materialized view (for the runtime loop's fold).
    pub fn view_mut(&mut self) -> &mut MaterializedView {
        &mut self.view
    }

    /// Mutable access to the cost breaker (for the runtime loop's observe).
    pub fn breaker_mut(&mut self) -> &mut CostBreaker {
        &mut self.breaker
    }

    /// Mutable access to the stream hub (for the runtime loop's publish).
    pub fn hub_mut(&mut self) -> &mut StreamHub {
        &mut self.hub
    }

    /// Mutable access to the state reader (for inspection endpoints).
    pub fn inspect_mut(&mut self) -> &mut StateReader {
        &mut self.inspect
    }

    /// Mark an agent's state as dirty — SSE producers will emit an
    /// `invalidate` event on the next sweep.
    pub fn mark_dirty(&mut self, agent_id: &str) {
        let _new = self.dirty_agents.insert(agent_id.to_owned());
    }

    /// Check and clear the dirty flag for an agent. Returns `true` if the
    /// agent was dirty (the caller should emit an `invalidate` SSE event).
    pub fn take_dirty(&mut self, agent_id: &str) -> bool {
        self.dirty_agents.remove(agent_id)
    }

    /// Construct a backend from explicit services — used by tests.
    #[cfg(test)]
    pub(crate) fn for_test(agents_dir: PathBuf, view: MaterializedView, breaker: CostBreaker) -> Self {
        Self {
            view,
            breaker,
            hub: StreamHub::new(DEFAULT_SUB_CAPACITY),
            tickets: TicketStore::new(),
            inspect: StateReader::new(),
            retired: RetiredStore::default(),
            names: NameOverrides::default(),
            avatars: AvatarStore::default(),
            agents_dir,
            dirty_agents: HashSet::new(),
            supervisor: AgentSupervisor::new(&[]),
            agents_root: PathBuf::from("/tmp/cp-test-realms"),
            agent_binary: PathBuf::from("/tmp/cp-test-bin"),
            auth: None,
            session_ttl: Duration::from_secs(3600),
            env_overrides: HashMap::new(),
        }
    }
}
