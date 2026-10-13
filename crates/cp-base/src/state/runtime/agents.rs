//! Thread-runtime storage, keyed by agent id.
//!
//! Every agent's [`AgentRuntime`] lives here permanently. "Which agent is
//! executing" is just an id ([`set_executing`](AgentStore::set_executing));
//! nothing is moved when it changes.
//!
//! [`State::agent`](super::State::agent) returns [`current`](AgentStore::current): the
//! executing agent's runtime, or the *unbound* runtime when no stored agent
//! is executing (cold boot before the first focus, or focus cleared). Boot
//! assembles the focused agent's data into the unbound runtime;
//! [`take_unbound`](AgentStore::take_unbound) hands it to its agent.

use super::bundle::AgentRuntime;

/// One stored agent runtime.
#[derive(Debug)]
struct Slot {
    /// Owning agent id.
    id: String,
    /// The agent's whole per-agent context.
    runtime: AgentRuntime,
}

/// Owner of every agent's [`AgentRuntime`] plus the executing-thread id.
#[derive(Debug, Default)]
pub struct AgentStore {
    /// Stored agent contexts. A `Vec` (not a map): a handful of agents, and
    /// the hot `State::agent()` path is a cached index, not a hash.
    slots: Vec<Slot>,
    /// Cached index into `slots` of the executing agent (`None` = unbound).
    cursor: Option<usize>,
    /// Context used when no stored agent is executing.
    unbound: AgentRuntime,
    /// The executing agent's id (see [`executing`](Self::executing)).
    executing: Option<String>,
}

impl AgentStore {
    /// The agent executing right now: owner of every per-agent write.
    /// `None` when no agent is placed (cold boot, focus cleared).
    #[must_use]
    pub fn executing(&self) -> Option<&str> {
        self.executing.as_deref()
    }

    /// Make `id` the executing agent (`None` = the unbound runtime). No data
    /// moves. An id without a stored runtime resolves to the unbound runtime
    /// until one is inserted.
    pub fn set_executing(&mut self, id: Option<String>) {
        self.executing = id;
        self.resolve_cursor();
    }

    /// Store `runtime` as agent `id`'s context, replacing any previous one.
    pub fn insert(&mut self, id: String, runtime: AgentRuntime) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.id == id) {
            slot.runtime = runtime;
        } else {
            self.slots.push(Slot { id, runtime });
        }
        self.resolve_cursor();
    }

    /// Drop agent `id`'s stored context. Returns it, if any.
    pub fn remove(&mut self, id: &str) -> Option<AgentRuntime> {
        let pos = self.slots.iter().position(|s| s.id == id)?;
        let slot = self.slots.swap_remove(pos);
        self.resolve_cursor();
        Some(slot.runtime)
    }

    /// Whether agent `id` has a stored context.
    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.slots.iter().any(|s| s.id == id)
    }

    /// Ids of every agent with a stored context.
    #[must_use]
    pub fn ids(&self) -> Vec<String> {
        self.slots.iter().map(|s| s.id.clone()).collect()
    }

    /// Read-only access to a stored agent's context, by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&AgentRuntime> {
        self.slots.iter().find(|s| s.id == id).map(|s| &s.runtime)
    }

    /// Mutable access to a stored agent's context, by id.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut AgentRuntime> {
        self.slots.iter_mut().find(|s| s.id == id).map(|s| &mut s.runtime)
    }

    /// Take the unbound runtime (boot data or a blank), leaving `replacement`.
    pub const fn take_unbound(&mut self, replacement: AgentRuntime) -> AgentRuntime {
        std::mem::replace(&mut self.unbound, replacement)
    }

    /// The runtime `State` currently derefs to.
    #[must_use]
    pub fn current(&self) -> &AgentRuntime {
        self.cursor.and_then(|i| self.slots.get(i)).map_or(&self.unbound, |s| &s.runtime)
    }

    /// Mutable twin of [`current`](Self::current).
    pub fn current_mut(&mut self) -> &mut AgentRuntime {
        match self.cursor.and_then(|i| self.slots.get_mut(i)) {
            Some(slot) => &mut slot.runtime,
            None => &mut self.unbound,
        }
    }

    /// Recompute the cached slot index of the executing agent.
    fn resolve_cursor(&mut self) {
        self.cursor = self.executing.as_deref().and_then(|id| self.slots.iter().position(|s| s.id == id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executing_id_selects_runtime_without_moving_data() {
        let mut store = AgentStore::default();
        let mut a = AgentRuntime::new();
        a.next_user_id = 7;
        store.insert("A".to_owned(), a);
        assert_eq!(store.current().next_user_id, 1, "nothing executing means unbound");
        store.set_executing(Some("A".to_owned()));
        assert_eq!(store.current().next_user_id, 7);
        store.current_mut().next_user_id = 8;
        store.set_executing(None);
        assert_eq!((store.current().next_user_id, store.get("A").map(|r| r.next_user_id)), (1, Some(8)));
    }

    #[test]
    fn cursor_survives_removal_of_other_slot() {
        let mut store = AgentStore::default();
        store.insert("A".to_owned(), AgentRuntime::new());
        let mut b = AgentRuntime::new();
        b.next_user_id = 5;
        store.insert("B".to_owned(), b);
        store.set_executing(Some("B".to_owned()));
        drop(store.remove("A"));
        assert_eq!(store.current().next_user_id, 5);
    }
}
