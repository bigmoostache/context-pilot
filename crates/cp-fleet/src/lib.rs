//! Fleet mechanism — the I/O-free core of Context Pilot's multi-thread model.
//!
//! A **thread** is the unit of execution: it owns its context and its execution
//! state. This crate holds the *mechanism* that lets one cooperative loop drive
//! many threads at once, without any knowledge of what a thread's context
//! actually contains:
//!
//! - [`ThreadExecState`] — where a thread sits in the loop.
//! - [`Role`] — whether a registry entry is a peer thread or a subordinate
//!   reverie (only peer threads count against the concurrency cap).
//! - [`FleetRegistry`] — a generic table of entries, parameterised over the
//!   concrete per-thread runtime `R` (defined in `cp-base` to avoid a
//!   dependency cycle: this crate must not depend on `cp-base`).
//! - [`K`] — the concurrency cap, and [`promote`] — the starvation-free policy
//!   that fills free active slots from the waiting queue.
//!
//! The split is deliberate: the *bundle* of per-thread state (`cp-base`'s
//! `ThreadRuntime`) holds editor/conversation/cache types that live in
//! `cp-base`; this crate stays dependency-light and holds only the vocabulary
//! and the scheduling policy, generic over that bundle.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Maximum number of threads that may be *actively executing* at once.
///
/// This is the only thing left of the old fixed "worker pool": a bound on
/// concurrency (LLM streams in flight, cost/rate-limit pressure), demoted from
/// an entity to a number. A thread beyond this bound is `Runnable` but waits for
/// a free slot (see [`promote`]).
pub const K: usize = 2;

/// Where a thread sits in the cooperative loop.
///
/// A thread is *active* (occupies a concurrency slot) only while it is driving
/// the LLM — i.e. [`Streaming`](Self::Streaming) /
/// [`AwaitingLlm`](Self::AwaitingLlm). A thread parked on a background tool
/// yields its slot ([`AwaitingTool`](Self::AwaitingTool)) and re-enters the
/// runnable queue when the result lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThreadExecState {
    /// No work to do — the thread is `THEIR_TURN` or finished. Not scheduled.
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
    /// so the thread releases its slot until the tool's watcher fires.
    AwaitingTool,
    /// The last turn failed (guard rail, API error). Surfaced in the UI; not
    /// scheduled until re-engaged.
    Errored,
}

impl ThreadExecState {
    /// Whether this state occupies a concurrency slot (counts against [`K`]).
    ///
    /// Only states with an in-flight LLM turn are active; everything else —
    /// including `AwaitingTool` — is free of the cap.
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Streaming | Self::AwaitingLlm)
    }

    /// Whether this thread has work ready and is waiting only for a free slot.
    #[must_use]
    pub const fn is_runnable(self) -> bool {
        matches!(self, Self::Runnable)
    }
}

/// The role a registry entry plays in the fleet.
///
/// There is one execution primitive; `Role` distinguishes its two uses. Only a
/// [`Thread`](Self::Thread) counts against the concurrency cap [`K`]; a
/// [`Reverie`](Self::Reverie) is a background subordinate scoped to a parent and
/// is never gated by the cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Role {
    /// A user-facing peer thread. Persistent; counts against [`K`].
    #[default]
    Thread,
    /// A background subordinate (cleaner, cartographer). Transient; not capped.
    Reverie,
}

impl Role {
    /// Whether entries of this role are subject to the concurrency cap [`K`].
    #[must_use]
    pub const fn is_capped(self) -> bool {
        matches!(self, Self::Thread)
    }
}

/// One entry in the [`FleetRegistry`]: scheduling metadata plus the concrete
/// per-thread runtime bundle `R`.
///
/// `R` is supplied by the owning crate (`cp-base`'s `ThreadRuntime`) so this
/// crate need not know what a thread's context contains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry<R> {
    /// Peer thread or subordinate reverie.
    pub role: Role,
    /// Current position in the cooperative loop.
    pub exec_state: ThreadExecState,
    /// Monotonic timestamp (ms) at which this entry last became `Runnable` while
    /// waiting for a slot. Used by [`promote`] for oldest-waiting-first order.
    /// `None` when the entry is not waiting.
    pub waiting_since_ms: Option<u64>,
    /// The concrete per-thread runtime (conversation, context, budget, …).
    pub runtime: R,
}

impl<R> Entry<R> {
    /// A fresh entry wrapping `runtime` with the given `role`, `Idle` and not
    /// waiting.
    pub const fn new(role: Role, runtime: R) -> Self {
        Self { role, exec_state: ThreadExecState::Idle, waiting_since_ms: None, runtime }
    }
}

/// A generic table of fleet entries keyed by thread id.
///
/// Holds the *mechanism* (membership, counts, lookup) while remaining ignorant
/// of the runtime payload `R`. Scheduling policy lives in [`promote`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetRegistry<R> {
    /// Entries keyed by thread id.
    entries: HashMap<String, Entry<R>>,
}

impl<R> Default for FleetRegistry<R> {
    fn default() -> Self {
        Self { entries: HashMap::new() }
    }
}

impl<R> FleetRegistry<R> {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace the entry for `id`.
    pub fn insert(&mut self, id: String, entry: Entry<R>) {
        let _prev = self.entries.insert(id, entry);
    }

    /// Remove and return the entry for `id`, if present. Retiring a thread this
    /// way drops its runtime (and, by construction, every resource the runtime
    /// owns).
    pub fn remove(&mut self, id: &str) -> Option<Entry<R>> {
        self.entries.remove(id)
    }

    /// Shared access to an entry.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Entry<R>> {
        self.entries.get(id)
    }

    /// Mutable access to an entry.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Entry<R>> {
        self.entries.get_mut(id)
    }

    /// Whether an entry exists for `id`.
    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    /// Number of entries (threads + reveries).
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
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Entry<R>)> {
        self.entries.iter()
    }

    /// Iterate all `(id, entry)` pairs mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&String, &mut Entry<R>)> {
        self.entries.iter_mut()
    }

    /// Count of currently-active capped entries — i.e. peer threads occupying a
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

/// Decide which waiting-runnable threads to promote into free active slots.
///
/// Returns the ids to promote this tick: the oldest-waiting runnable capped
/// threads, up to the number of free slots (`K - active`). A thread is its own
/// executor, so there is nothing to arbitrate — promotion only picks from the
/// waiting queue. Reveries are never promoted here (they are not capped).
#[must_use]
pub fn promote<R>(registry: &FleetRegistry<R>) -> Vec<String> {
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

    /// A trivial runtime payload for tests.
    #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
    struct TestRt {
        label: String,
    }

    fn rt(label: &str) -> TestRt {
        TestRt { label: label.to_owned() }
    }

    #[test]
    fn active_state_classification() {
        assert!(ThreadExecState::Streaming.is_active());
        assert!(ThreadExecState::AwaitingLlm.is_active());
        assert!(!ThreadExecState::AwaitingTool.is_active());
        assert!(!ThreadExecState::Runnable.is_active());
        assert!(!ThreadExecState::Idle.is_active());
        assert!(!ThreadExecState::Errored.is_active());
    }

    #[test]
    fn reverie_is_not_capped() {
        assert!(Role::Thread.is_capped());
        assert!(!Role::Reverie.is_capped());
    }

    #[test]
    fn registry_basic_ops() {
        let mut reg: FleetRegistry<TestRt> = FleetRegistry::new();
        assert!(reg.is_empty());
        reg.insert("T1".to_owned(), Entry::new(Role::Thread, rt("a")));
        assert_eq!(reg.len(), 1);
        assert!(reg.contains("T1"));
        assert_eq!(reg.get("T1").map(|e| e.runtime.label.as_str()), Some("a"));
        let removed = reg.remove("T1");
        assert_eq!(removed.map(|e| e.runtime.label), Some("a".to_owned()));
        assert!(reg.is_empty());
    }

    #[test]
    fn active_count_ignores_reveries_and_parked() {
        let mut reg: FleetRegistry<TestRt> = FleetRegistry::new();
        let mut t1 = Entry::new(Role::Thread, rt("t1"));
        t1.exec_state = ThreadExecState::Streaming;
        let mut rev = Entry::new(Role::Reverie, rt("rev"));
        rev.exec_state = ThreadExecState::Streaming; // active but not capped
        let mut t2 = Entry::new(Role::Thread, rt("t2"));
        t2.exec_state = ThreadExecState::AwaitingTool; // parked, not active
        reg.insert("T1".to_owned(), t1);
        reg.insert("R1".to_owned(), rev);
        reg.insert("T2".to_owned(), t2);
        assert_eq!(reg.active_capped_count(), 1);
    }

    #[test]
    fn promote_fills_free_slots_oldest_first() {
        let mut reg: FleetRegistry<TestRt> = FleetRegistry::new();
        // One active thread → one free slot (K=2).
        let mut active = Entry::new(Role::Thread, rt("active"));
        active.exec_state = ThreadExecState::Streaming;
        reg.insert("A".to_owned(), active);
        // Two waiting runnable threads with different wait times.
        let mut w_new = Entry::new(Role::Thread, rt("new"));
        w_new.exec_state = ThreadExecState::Runnable;
        w_new.waiting_since_ms = Some(200);
        let mut w_old = Entry::new(Role::Thread, rt("old"));
        w_old.exec_state = ThreadExecState::Runnable;
        w_old.waiting_since_ms = Some(100);
        reg.insert("WNEW".to_owned(), w_new);
        reg.insert("WOLD".to_owned(), w_old);

        let promoted = promote(&reg);
        assert_eq!(promoted, vec!["WOLD".to_owned()], "oldest-waiting first, only one free slot");
    }

    #[test]
    fn promote_none_when_full() {
        let mut reg: FleetRegistry<TestRt> = FleetRegistry::new();
        for id in ["A", "B"] {
            let mut e = Entry::new(Role::Thread, rt(id));
            e.exec_state = ThreadExecState::Streaming;
            reg.insert(id.to_owned(), e);
        }
        let mut waiting = Entry::new(Role::Thread, rt("w"));
        waiting.exec_state = ThreadExecState::Runnable;
        waiting.waiting_since_ms = Some(1);
        reg.insert("W".to_owned(), waiting);
        assert!(promote(&reg).is_empty(), "no free slots at K=2");
    }
}
