//! Fleet mechanism — the I/O-free core of Context Pilot's multi-agent model.
//!
//! An **agent** is the unit of execution: it owns its context and its execution
//! state. This crate holds the *mechanism* that lets one cooperative loop drive
//! many agents at once, without any knowledge of what an agent's context
//! actually contains:
//!
//! - [`AgentExecState`] — where an agent sits in the loop, and
//!   [`exec_state_from_facts`] — how a step's outcome maps onto it.
//! - [`FleetRegistry`] — scheduling metadata per background agent id. Agent
//!   runtimes live in `cp-base`'s `AgentStore`; this crate must not depend on
//!   `cp-base`.
//! - [`K`] — the default concurrency cap, and [`promote`] — the
//!   starvation-free policy that fills free active slots from the waiting queue.
//!
//! The split is deliberate: the *bundle* of per-agent state (`cp-base`'s
//! `AgentRuntime`) holds editor/conversation/cache types that live in
//! `cp-base`; this crate stays dependency-free and holds only the vocabulary
//! and the scheduling policy.

use std::collections::HashMap;

/// Default bound on the number of agents *actively executing* (LLM stream in
/// flight) at once, the focused agent included.
///
/// The only thing left of the old fixed "worker pool": a bound on concurrent
/// LLM streams, demoted from an entity to a number. It does not meter cost or
/// tokens. An agent beyond the bound stays `Runnable` and waits for a free slot
/// (see [`promote`], which takes the cap as an argument).
///
/// Set deliberately high (user decision T864): a dozen agents should all be
/// able to continue on their own without the human focusing each one.
pub const K: usize = 50;

/// Consecutive terminal stream failures (each after its own API retries are
/// exhausted) before an agent is parked as [`Errored`](AgentExecState::Errored).
///
/// A one-off blip stays `Runnable` and retries after the spine's backoff; only
/// an agent that keeps failing is declared *stuck* and removed from scheduling
/// until a human re-engages it (design doc §8).
pub const STUCK_ERROR_THRESHOLD: usize = 3;

/// Where an agent sits in the cooperative loop.
///
/// An agent is *active* (occupies a concurrency slot) only while
/// [`Streaming`](Self::Streaming). An agent parked on pending tools or a
/// blocking wait stays `Streaming` too: the step loop is what polls its
/// watchers, so it must keep being stepped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentExecState {
    /// No work to do — the agent is `THEIR_TURN` or finished. Not scheduled.
    #[default]
    Idle,
    /// Has queued work (its turn, a fresh notification) and is eligible to be
    /// advanced once it holds a slot.
    Runnable,
    /// Mid-turn: a stream, its tools or a blocking wait are in flight. Holds a
    /// slot and is stepped every tick.
    Streaming,
    /// Repeated terminal failures ([`STUCK_ERROR_THRESHOLD`]). Surfaced in the
    /// UI; not scheduled until a human re-engages it.
    Errored,
}

impl AgentExecState {
    /// Whether this state occupies a concurrency slot (counts against the cap).
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Streaming)
    }

    /// Whether this agent has work ready and is waiting only for a free slot.
    #[must_use]
    pub const fn is_runnable(self) -> bool {
        matches!(self, Self::Runnable)
    }
}

/// The state an agent lands in after a step, from the facts the step left:
/// a live stream wins, then a stuck error streak, then the agent's turn.
#[must_use]
pub const fn exec_state_from_facts(is_streaming: bool, consecutive_errors: usize, my_turn: bool) -> AgentExecState {
    if is_streaming {
        AgentExecState::Streaming
    } else if consecutive_errors >= STUCK_ERROR_THRESHOLD {
        AgentExecState::Errored
    } else if my_turn {
        AgentExecState::Runnable
    } else {
        AgentExecState::Idle
    }
}

/// One entry in the [`FleetRegistry`]: a background agent's scheduling
/// metadata.
///
/// The agent's runtime is not here: it lives permanently in `cp-base`'s
/// `AgentStore`, keyed by the same agent id.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Entry {
    /// Current position in the cooperative loop.
    exec_state: AgentExecState,
    /// Registry sequence number taken when this entry last became `Runnable`;
    /// `None` when it is not waiting. A logical clock, so wall-clock jumps
    /// (NTP, sleep/wake) cannot reorder the queue. Orders [`promote`].
    waiting_seq: Option<u64>,
}

impl Entry {
    /// A fresh entry: `Idle` and not waiting.
    #[must_use]
    pub const fn new() -> Self {
        Self { exec_state: AgentExecState::Idle, waiting_seq: None }
    }

    /// Current execution state.
    #[must_use]
    pub const fn exec_state(&self) -> AgentExecState {
        self.exec_state
    }

    /// Queue position stamp, `None` unless `Runnable`.
    #[must_use]
    pub const fn waiting_seq(&self) -> Option<u64> {
        self.waiting_seq
    }
}

/// Scheduling metadata keyed by agent id. The focused agent is never in it.
///
/// State changes go through [`set_state`](Self::set_state), which owns the
/// waiting-queue stamp: an entry is stamped exactly on its edge *into*
/// `Runnable` and cleared on its edge out, so re-deriving `Runnable` keeps an
/// agent's place and an agent that just ran re-queues behind the others.
#[derive(Debug, Clone, Default)]
pub struct FleetRegistry {
    /// Entries keyed by agent id.
    entries: HashMap<String, Entry>,
    /// Next waiting-queue stamp.
    next_seq: u64,
}

impl FleetRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert `id` as a fresh entry in `state` (stamped if `Runnable`),
    /// replacing any previous entry.
    pub fn insert(&mut self, id: String, state: AgentExecState) {
        let mut entry = Entry::new();
        self.transition(&mut entry, state);
        let _prev = self.entries.insert(id, entry);
    }

    /// Insert a fresh `Idle` entry for `id` unless one exists.
    pub fn ensure(&mut self, id: &str) {
        if !self.entries.contains_key(id) {
            let _prev = self.entries.insert(id.to_owned(), Entry::new());
        }
    }

    /// Move `id` to `state`, maintaining its waiting stamp. Returns `false`
    /// when `id` has no entry.
    pub fn set_state(&mut self, id: &str, state: AgentExecState) -> bool {
        let Some(mut entry) = self.entries.get(id).copied() else {
            return false;
        };
        self.transition(&mut entry, state);
        let _prev = self.entries.insert(id.to_owned(), entry);
        true
    }

    /// Re-derive a reconciled entry's state from its turn: `MyTurn` →
    /// `Runnable`, else `Idle`. Active entries belong to the step loop and
    /// `Errored` ones wait for a human, so both are left untouched. A no-op
    /// for an unknown `id`.
    pub fn derive_from_turn(&mut self, id: &str, my_turn: bool) {
        let Some(current) = self.entries.get(id).map(|entry| entry.exec_state) else {
            return;
        };
        if current.is_active() || current == AgentExecState::Errored {
            return;
        }
        let next = if my_turn { AgentExecState::Runnable } else { AgentExecState::Idle };
        let _known = self.set_state(id, next);
    }

    /// Apply `state` to `entry`, stamping on the edge into `Runnable` and
    /// clearing the stamp on any other state.
    const fn transition(&mut self, entry: &mut Entry, state: AgentExecState) {
        if state.is_runnable() {
            if entry.waiting_seq.is_none() {
                entry.waiting_seq = Some(self.next_seq);
                self.next_seq = self.next_seq.wrapping_add(1);
            }
        } else {
            entry.waiting_seq = None;
        }
        entry.exec_state = state;
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

    /// Whether an entry exists for `id`.
    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    /// Number of entries.
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

    /// Ids of entries in `state`, cloned so the caller may mutate while
    /// walking them.
    #[must_use]
    pub fn ids_in(&self, state: AgentExecState) -> Vec<String> {
        self.entries.iter().filter(|pair| pair.1.exec_state == state).map(|pair| pair.0.clone()).collect()
    }

    /// Count of entries occupying a concurrency slot.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.entries.values().filter(|entry| entry.exec_state.is_active()).count()
    }
}

/// Decide which waiting agents to promote into free active slots this tick.
///
/// `cap` is the concurrency bound (normally [`K`]); `external_active` counts
/// slots held outside the registry (the focused agent while it streams).
/// Returns at most `cap - active` ids, oldest-waiting first, ties by id.
/// Allocates nothing when no slot is free or nobody waits.
#[must_use]
pub fn promote(registry: &FleetRegistry, cap: usize, external_active: usize) -> Vec<String> {
    let free = cap.saturating_sub(registry.active_count().saturating_add(external_active));
    if free == 0 {
        return Vec::new();
    }
    let mut waiting: Vec<(u64, &String)> = registry
        .entries
        .iter()
        .filter(|pair| pair.1.exec_state.is_runnable())
        .map(|pair| (pair.1.waiting_seq.unwrap_or(u64::MAX), pair.0))
        .collect();
    if waiting.is_empty() {
        return Vec::new();
    }
    if waiting.len() > free {
        let _partitioned = waiting.select_nth_unstable(free.saturating_sub(1));
        waiting.truncate(free);
    }
    waiting.sort_unstable();
    waiting.into_iter().map(|pair| pair.1.clone()).collect()
}

#[cfg(test)]
mod tests;
