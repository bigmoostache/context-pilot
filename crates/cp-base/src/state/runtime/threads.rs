//! Thread-runtime storage, keyed by thread id.
//!
//! Every thread's [`ThreadRuntime`] lives here permanently. "Which thread is
//! executing" is just an id ([`set_executing`](ThreadStore::set_executing));
//! nothing is moved when it changes.
//!
//! [`State`](super::State) derefs to [`current`](ThreadStore::current): the
//! executing thread's runtime, or the *unbound* runtime when no stored thread
//! is executing (cold boot before the first focus, or focus cleared). Boot
//! assembles the focused thread's data into the unbound runtime;
//! [`take_unbound`](ThreadStore::take_unbound) hands it to its thread.

use super::bundle::ThreadRuntime;

/// One stored thread runtime.
#[derive(Debug)]
struct Slot {
    /// Owning thread id.
    id: String,
    /// The thread's whole per-thread context.
    runtime: ThreadRuntime,
}

/// Owner of every thread's [`ThreadRuntime`] plus the executing-thread id.
#[derive(Debug, Default)]
pub struct ThreadStore {
    /// Stored thread contexts. A `Vec` (not a map): a handful of threads, and
    /// the hot `State::thread()` path is a cached index, not a hash.
    slots: Vec<Slot>,
    /// Cached index into `slots` of the executing thread (`None` = unbound).
    cursor: Option<usize>,
    /// Context used when no stored thread is executing.
    unbound: ThreadRuntime,
    /// The executing thread's id (see [`executing`](Self::executing)).
    executing: Option<String>,
}

impl ThreadStore {
    /// The thread executing right now: owner of every per-thread write.
    /// `None` when no thread is placed (cold boot, focus cleared).
    #[must_use]
    pub fn executing(&self) -> Option<&str> {
        self.executing.as_deref()
    }

    /// Make `id` the executing thread (`None` = the unbound runtime). No data
    /// moves. An id without a stored runtime resolves to the unbound runtime
    /// until one is inserted.
    pub fn set_executing(&mut self, id: Option<String>) {
        self.executing = id;
        self.resolve_cursor();
    }

    /// Store `runtime` as thread `id`'s context, replacing any previous one.
    pub fn insert(&mut self, id: String, runtime: ThreadRuntime) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.id == id) {
            slot.runtime = runtime;
        } else {
            self.slots.push(Slot { id, runtime });
        }
        self.resolve_cursor();
    }

    /// Drop thread `id`'s stored context. Returns it, if any.
    pub fn remove(&mut self, id: &str) -> Option<ThreadRuntime> {
        let pos = self.slots.iter().position(|s| s.id == id)?;
        let slot = self.slots.swap_remove(pos);
        self.resolve_cursor();
        Some(slot.runtime)
    }

    /// Whether thread `id` has a stored context.
    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.slots.iter().any(|s| s.id == id)
    }

    /// Ids of every thread with a stored context.
    #[must_use]
    pub fn ids(&self) -> Vec<String> {
        self.slots.iter().map(|s| s.id.clone()).collect()
    }

    /// Read-only access to a stored thread's context, by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&ThreadRuntime> {
        self.slots.iter().find(|s| s.id == id).map(|s| &s.runtime)
    }

    /// Mutable access to a stored thread's context, by id.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut ThreadRuntime> {
        self.slots.iter_mut().find(|s| s.id == id).map(|s| &mut s.runtime)
    }

    /// Take the unbound runtime (boot data or a blank), leaving `replacement`.
    pub const fn take_unbound(&mut self, replacement: ThreadRuntime) -> ThreadRuntime {
        std::mem::replace(&mut self.unbound, replacement)
    }

    /// The runtime `State` currently derefs to.
    #[must_use]
    pub fn current(&self) -> &ThreadRuntime {
        self.cursor.and_then(|i| self.slots.get(i)).map_or(&self.unbound, |s| &s.runtime)
    }

    /// Mutable twin of [`current`](Self::current).
    pub fn current_mut(&mut self) -> &mut ThreadRuntime {
        match self.cursor.and_then(|i| self.slots.get_mut(i)) {
            Some(slot) => &mut slot.runtime,
            None => &mut self.unbound,
        }
    }

    /// Recompute the cached slot index of the executing thread.
    fn resolve_cursor(&mut self) {
        self.cursor = self.executing.as_deref().and_then(|id| self.slots.iter().position(|s| s.id == id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executing_id_selects_runtime_without_moving_data() {
        let mut store = ThreadStore::default();
        let mut a = ThreadRuntime::new();
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
        let mut store = ThreadStore::default();
        store.insert("A".to_owned(), ThreadRuntime::new());
        let mut b = ThreadRuntime::new();
        b.next_user_id = 5;
        store.insert("B".to_owned(), b);
        store.set_executing(Some("B".to_owned()));
        drop(store.remove("A"));
        assert_eq!(store.current().next_user_id, 5);
    }
}
