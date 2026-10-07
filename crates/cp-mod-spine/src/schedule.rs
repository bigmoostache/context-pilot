//! Fleet-shared coucou registry.
//!
//! Coucous used to live as `CoucouWatcher`s in each thread's own
//! `WatcherRegistry`, which is only polled while that thread is stepped — so a
//! background thread idle on THEIR_TURN never fired its reminders. They now
//! live here, ONE instance for the whole fleet (in `shared_module_data`),
//! polled once per tick by the app loop and delivered into each coucou's
//! target thread inbox.

use serde::{Deserialize, Serialize};

use cp_base::state::runtime::State;

use crate::coucou::Record;

/// Every pending coucou of the fleet plus the id counter.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CoucouRegistry {
    /// Pending coucous, in scheduling order.
    pub pending: Vec<Record>,
    /// Next numeric suffix for `coucou_<n>` ids. Persisted, so ids stay unique
    /// across reloads (a process-local counter restarted at 0 and collided).
    pub next_id: u64,
}

/// A coucou that came due this tick, ready for delivery.
#[derive(Debug, Clone)]
pub struct FiredCoucou {
    /// Coucou id (`coucou_<n>`), used as the notification source for dedup.
    pub id: String,
    /// Target thread; `None` = whichever thread is focused.
    pub thread_id: Option<String>,
    /// Notification text.
    pub description: String,
}

impl CoucouRegistry {
    /// Shared registry from `state`.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Mutable shared registry from `state`.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }

    /// Allocate a fresh `coucou_<n>` id.
    pub fn alloc_id(&mut self) -> String {
        let id = format!("coucou_{}", self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Raise `next_id` above every pending id, so restored or migrated records
    /// can never collide with freshly allocated ones.
    pub fn reseed_counter(&mut self) {
        let max_seen = self
            .pending
            .iter()
            .filter_map(|c| c.watcher_id.strip_prefix("coucou_")?.parse::<u64>().ok())
            .max()
            .map_or(0, |n| n.saturating_add(1));
        self.next_id = self.next_id.max(max_seen);
    }

    /// Remove the coucou with `id`. Returns whether one was removed.
    pub fn cancel(&mut self, id: &str) -> bool {
        let before = self.pending.len();
        self.pending.retain(|c| c.watcher_id != id);
        self.pending.len() < before
    }

    /// Whether any coucou targets `thread_id` (or is unscoped). Drives the
    /// WAITING badge of the focused thread.
    #[must_use]
    pub fn has_pending_for(&self, thread_id: Option<&str>) -> bool {
        self.pending.iter().any(|c| c.thread_id.is_none() || c.thread_id.as_deref() == thread_id)
    }

    /// Pop every coucou due at `now`: one-shots are removed, recurrent ones are
    /// re-armed at `now + interval`.
    pub fn take_due(&mut self, now: u64) -> Vec<FiredCoucou> {
        let mut fired = Vec::new();
        self.pending.retain_mut(|c| {
            if now < c.fire_at_ms {
                return true;
            }
            fired.push(FiredCoucou {
                id: c.watcher_id.clone(),
                thread_id: c.thread_id.clone(),
                description: c.thread_id.as_ref().map_or_else(
                    || format!("⏰ Coucou! {}", c.message),
                    |tid| format!("⏰ Coucou (thread {tid})! {}", c.message),
                ),
            });
            if c.interval_ms > 0 {
                c.fire_at_ms = now.saturating_add(c.interval_ms);
                true
            } else {
                false
            }
        });
        fired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(id: &str, fire_at_ms: u64, interval_ms: u64) -> Record {
        Record {
            watcher_id: id.to_owned(),
            message: "m".to_owned(),
            registered_at_ms: 0,
            fire_at_ms,
            thread_id: Some("T1".to_owned()),
            interval_ms,
            recurrence_label: None,
        }
    }

    #[test]
    fn take_due_drops_one_shot_and_rearms_recurrent() {
        let mut reg = CoucouRegistry { pending: vec![data("coucou_0", 10, 0), data("coucou_1", 10, 100)], next_id: 2 };
        assert!(reg.take_due(5).is_empty());
        let fired = reg.take_due(10);
        assert_eq!(fired.len(), 2);
        assert_eq!(reg.pending.len(), 1);
        assert_eq!(reg.pending.first().map(|c| c.fire_at_ms), Some(110));
    }

    #[test]
    fn reseed_avoids_collisions() {
        let mut reg = CoucouRegistry { pending: vec![data("coucou_7", 0, 0)], next_id: 0 };
        reg.reseed_counter();
        assert_eq!(reg.alloc_id(), "coucou_8");
    }
}
