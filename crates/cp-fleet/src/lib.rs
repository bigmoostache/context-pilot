//! Fleet mechanism — the I/O-free core of Context Pilot's multi-thread model.
//!
//! A **agent** is the unit of execution: it owns its context and its execution
//! state. This crate holds the *mechanism* that lets one cooperative loop drive
//! many agents at once, without any knowledge of what an agent's context
//! actually contains:
//!
//! - [`AgentExecState`] — where an agent sits in the loop.
//! - [`Role`] — whether a registry entry is a peer agent or a subordinate
//!   reverie (only peer agents count against the concurrency cap).
//! - [`FleetRegistry`] — scheduling metadata per agent id. Agent runtimes
//!   live in `cp-base`'s `AgentStore`; this crate must not depend on `cp-base`.
//! - [`K`] — the concurrency cap, and [`promote`] — the starvation-free policy
//!   that fills free active slots from the waiting queue.
//!
//! The split is deliberate: the *bundle* of per-agent state (`cp-base`'s
//! `AgentRuntime`) holds editor/conversation/cache types that live in
//! `cp-base`; this crate stays dependency-light and holds only the vocabulary
//! and the scheduling policy.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Maximum number of agents that may be *actively executing* at once.
///
/// This is the only thing left of the old fixed "worker pool": a bound on
/// concurrency (LLM streams in flight, cost/rate-limit pressure), demoted from
/// an entity to a number. An agent beyond this bound is `Runnable` but waits for
/// a free slot (see [`promote`]).
///
/// Set deliberately high (user decision T864): a dozen agents should all be
/// able to continue on their own without the human focusing each one.
pub const K: usize = 50;

/// Where an agent sits in the cooperative loop.
///
/// An agent is *active* (occupies a concurrency slot) only while it is driving
/// the LLM — i.e. [`Streaming`](Self::Streaming) /
/// [`AwaitingLlm`](Self::AwaitingLlm). An agent parked on a background tool
/// yields its slot ([`AwaitingTool`](Self::AwaitingTool)) and re-enters the
/// runnable queue when the result lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AgentExecState {
    /// No work to do — the agent is `THEIR_TURN` or finished. Not scheduled.
    #[default]
    Idle,
    /// Has queued work (a chunk to consume, a tool result to apply, a fresh
    /// notification) and is eligible to be advanced once it holds a slot.
    Runnable,
    /// A request/stream is in flight; parked until the next chunk. Holds a slot.
    Streaming,
    /// Waiting for the LLM to begin responding. Holds a slot.
    AwaitingLlm,
    /// A background tool (console/callback/web/OCR) is running; the LLM is idle,
    /// so the agent releases its slot until the tool's watcher fires.
    AwaitingTool,
    /// The last turn failed (guard rail, API error). Surfaced in the UI; not
    /// scheduled until re-engaged.
    Errored,
}

impl AgentExecState {
    /// Whether this state occupies a concurrency slot (counts against [`K`]).
    ///
    /// Only states with an in-flight LLM turn are active; everything else —
    /// including `AwaitingTool` — is free of the cap.
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Streaming | Self::AwaitingLlm)
    }

    /// Whether this agent has work ready and is waiting only for a free slot.
    #[must_use]
    pub const fn is_runnable(self) -> bool {
        matches!(self, Self::Runnable)
    }
}

/// The role a registry entry plays in the fleet.
///
/// There is one execution primitive; `Role` distinguishes its two uses. Only a
/// [`Agent`](Self::Agent) counts against the concurrency cap [`K`]; a
/// [`Reverie`](Self::Reverie) is a background subordinate scoped to a parent and
/// is never gated by the cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Role {
    /// A user-facing peer agent. Persistent; counts against [`K`].
    #[default]
    #[serde(rename = "Thread")]
    Agent,
    /// A background subordinate (cleaner, cartographer). Transient; not capped.
    Reverie,
}

impl Role {
    /// Whether entries of this role are subject to the concurrency cap [`K`].
    #[must_use]
    pub const fn is_capped(self) -> bool {
        matches!(self, Self::Agent)
    }
}

/// One entry in the [`FleetRegistry`]: an agent's scheduling metadata.
///
/// The agent's runtime is not here: it lives permanently in `cp-base`'s
/// `AgentStore`, keyed by the same agent id.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Entry {
    /// Peer agent or subordinate reverie.
    pub role: Role,
    /// Current position in the cooperative loop.
    pub exec_state: AgentExecState,
    /// Monotonic timestamp (ms) at which this entry last became `Runnable` while
    /// waiting for a slot. Used by [`promote`] for oldest-waiting-first order.
    /// `None` when the entry is not waiting.
    pub waiting_since_ms: Option<u64>,
}

impl Entry {
    /// A fresh entry with the given `role`, `Idle` and not waiting.
    #[must_use]
    pub const fn new(role: Role) -> Self {
        Self { role, exec_state: AgentExecState::Idle, waiting_since_ms: None }
    }
}

/// A generic table of fleet entries keyed by agent id.
///
/// Holds the *mechanism* (membership, counts, lookup). Scheduling policy lives
/// in [`promote`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FleetRegistry {
    /// Entries keyed by agent id.
    entries: HashMap<String, Entry>,
}

impl FleetRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace the entry for `id`.
    pub fn insert(&mut self, id: String, entry: Entry) {
        let _prev = self.entries.insert(id, entry);
    }

    /// Remove and return the entry for `id`, if present. Scheduling metadata
    /// only: the agent's runtime is dropped via `AgentStore::remove`.
    pub fn remove(&mut self, id: &str) -> Option<Entry> {
        self.entries.remove(id)
    }

    /// Shared access to an entry.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.get(id)
    }

    /// Mutable access to an entry.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Entry> {
        self.entries.get_mut(id)
    }

    /// Whether an entry exists for `id`.
    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    /// Number of entries (agents + reveries).
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the registry holds no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate all `(id, entry)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Entry)> {
        self.entries.iter()
    }

    /// Iterate all `(id, entry)` pairs mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&String, &mut Entry)> {
        self.entries.iter_mut()
    }

    /// Count of currently-active capped entries — i.e. peer agents occupying a
    /// concurrency slot. Reveries never count (see [`Role::is_capped`]).
    #[must_use]
    pub fn active_capped_count(&self) -> usize {
        self.entries.values().filter(|e| e.role.is_capped() && e.exec_state.is_active()).count()
    }

    /// Ids of capped entries that are `Runnable` and waiting for a slot,
    /// ordered oldest-waiting-first (stable, starvation-free). Entries with no
    /// `waiting_since_ms` sort last.
    #[must_use]
    pub fn waiting_runnable_ids(&self) -> Vec<String> {
        let mut waiting: Vec<(&String, u64)> = self
            .entries
            .iter()
            .filter(|entry| entry.1.role.is_capped() && entry.1.exec_state.is_runnable())
            .map(|entry| (entry.0, entry.1.waiting_since_ms.unwrap_or(u64::MAX)))
            .collect();
        waiting.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(b.0)));
        waiting.into_iter().map(|(id, _)| id.clone()).collect()
    }
}

/// Decide which waiting-runnable agents to promote into free active slots.
///
/// Returns the ids to promote this tick: the oldest-waiting runnable capped
/// agents, up to the number of free slots (`K - active`). An agent is its own
/// executor, so there is nothing to arbitrate — promotion only picks from the
/// waiting queue. Reveries are never promoted here (they are not capped).
#[must_use]
pub fn promote(registry: &FleetRegistry) -> Vec<String> {
    let active = registry.active_capped_count();
    let free = K.saturating_sub(active);
    if free == 0 {
        return Vec::new();
    }
    let mut ids = registry.waiting_runnable_ids();
    ids.truncate(free);
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_state_classification() {
        assert!(AgentExecState::Streaming.is_active());
        assert!(AgentExecState::AwaitingLlm.is_active());
        assert!(!AgentExecState::AwaitingTool.is_active());
        assert!(!AgentExecState::Runnable.is_active());
        assert!(!AgentExecState::Idle.is_active());
        assert!(!AgentExecState::Errored.is_active());
    }

    #[test]
    fn reverie_is_not_capped() {
        assert!(Role::Agent.is_capped());
        assert!(!Role::Reverie.is_capped());
    }

    #[test]
    fn registry_basic_ops() {
        let mut reg = FleetRegistry::new();
        assert!(reg.is_empty());
        reg.insert("T1".to_owned(), Entry::new(Role::Agent));
        assert_eq!(reg.len(), 1);
        assert!(reg.contains("T1"));
        assert_eq!(reg.get("T1").map(|e| e.role), Some(Role::Agent));
        let removed = reg.remove("T1");
        assert_eq!(removed.map(|e| e.role), Some(Role::Agent));
        assert!(reg.is_empty());
    }

    #[test]
    fn active_count_ignores_reveries_and_parked() {
        let mut reg = FleetRegistry::new();
        let mut t1 = Entry::new(Role::Agent);
        t1.exec_state = AgentExecState::Streaming;
        let mut rev = Entry::new(Role::Reverie);
        rev.exec_state = AgentExecState::Streaming; // active but not capped
        let mut t2 = Entry::new(Role::Agent);
        t2.exec_state = AgentExecState::AwaitingTool; // parked, not active
        reg.insert("T1".to_owned(), t1);
        reg.insert("R1".to_owned(), rev);
        reg.insert("T2".to_owned(), t2);
        assert_eq!(reg.active_capped_count(), 1);
    }

    #[test]
    fn promote_fills_free_slots_oldest_first() {
        let mut reg = FleetRegistry::new();
        // K-1 active agents → exactly one free slot.
        for i in 1..K {
            let mut active = Entry::new(Role::Agent);
            active.exec_state = AgentExecState::Streaming;
            reg.insert(format!("A{i}"), active);
        }
        // Two waiting runnable agents with different wait times.
        let mut w_new = Entry::new(Role::Agent);
        w_new.exec_state = AgentExecState::Runnable;
        w_new.waiting_since_ms = Some(200);
        let mut w_old = Entry::new(Role::Agent);
        w_old.exec_state = AgentExecState::Runnable;
        w_old.waiting_since_ms = Some(100);
        reg.insert("WNEW".to_owned(), w_new);
        reg.insert("WOLD".to_owned(), w_old);

        let promoted = promote(&reg);
        assert_eq!(promoted, vec!["WOLD".to_owned()], "oldest-waiting first, only one free slot");
    }

    #[test]
    fn promote_none_when_full() {
        let mut reg = FleetRegistry::new();
        for i in 0..K {
            let mut e = Entry::new(Role::Agent);
            e.exec_state = AgentExecState::Streaming;
            reg.insert(format!("A{i}"), e);
        }
        let mut waiting = Entry::new(Role::Agent);
        waiting.exec_state = AgentExecState::Runnable;
        waiting.waiting_since_ms = Some(1);
        reg.insert("W".to_owned(), waiting);
        assert!(promote(&reg).is_empty(), "no free slots when K are active");
    }
}
