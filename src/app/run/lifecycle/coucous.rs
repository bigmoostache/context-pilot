//! Fleet-shared coucou delivery: poll the [`CoucouRegistry`] once per tick and
//! route each due reminder into its target thread's spine inbox, waking that
//! thread when it sits in the background. Plus the one-shot boot migration of
//! legacy per-thread `pending_coucous`.

use cp_mod_spine::schedule::CoucouRegistry;
use cp_mod_spine::types::{NotificationType, SpineState};
use cp_mod_threads::types::{FocusState, ThreadStatus, ThreadsState};

use crate::app::App;

#[expect(clippy::multiple_inherent_impl, reason = "App methods split across run/ submodules for readability")]
impl App {
    /// Deliver every coucou due now. Runs on the main loop with the focused
    /// thread resident (never inside a background step), so
    /// [`deliver_to_thread`](Self::deliver_to_thread) resolves "focused" against
    /// the real resident. Unscoped coucous (`thread_id: None`) go to the focused
    /// thread.
    ///
    /// A background target is flipped to `MyTurn`: its inbox notification alone
    /// does not schedule it (only `MyTurn` threads are promoted), so without the
    /// flip a `THEIR_TURN` thread would hold the reminder unread until the human
    /// returned to it.
    pub(super) fn check_coucous(&mut self) {
        let fired = CoucouRegistry::get_mut(&mut self.state).take_due(cp_base::panels::now_ms());
        if fired.is_empty() {
            return;
        }
        let focused = FocusState::get(&self.state).focused_thread_id.clone();
        for coucou in fired {
            let tid = coucou.thread_id.clone();
            let bind = tid.clone();
            // Source = coucou id: `create_notification` dedups on (kind, source),
            // so a shared source would swallow a second coucou firing together.
            self.deliver_to_thread(tid.as_deref(), |state| {
                let nid =
                    SpineState::create_notification(state, NotificationType::Custom, coucou.id, coucou.description);
                SpineState::set_notification_thread(state, &nid, bind);
            });
            if let Some(target) = tid.filter(|t| focused.as_deref() != Some(t.as_str()))
                && let Some(thread) = ThreadsState::get_mut(&mut self.state).threads.iter_mut().find(|t| t.id == target)
                && !thread.archived
            {
                thread.status = ThreadStatus::MyTurn;
            }
        }
        self.state.flags.ui.dirty = true;
        self.save_state_async();
    }

    /// Move coucous persisted by older builds in each thread's own spine slot
    /// into the fleet-shared registry. Each record keeps its explicit target, or
    /// is stamped with the thread that stored it, and gets a fresh id: the old
    /// per-thread counters all started at `coucou_0`, so the ids collide.
    /// Called once at boot, after background threads are loaded.
    pub(super) fn migrate_legacy_coucous(&mut self) {
        let focused = FocusState::get(&self.state).focused_thread_id.clone();
        let mut moved = drain_legacy(&mut self.state, focused.as_deref());

        let bg_ids: Vec<String> = self.fleet.iter().map(|entry| entry.0.clone()).collect();
        for id in bg_ids {
            self.deliver_to_thread(Some(&id), |state| moved.extend(drain_legacy(state, Some(&id))));
        }
        if moved.is_empty() {
            return;
        }
        let registry = CoucouRegistry::get_mut(&mut self.state);
        for mut coucou in moved {
            coucou.watcher_id = registry.alloc_id();
            registry.pending.push(coucou);
        }
        self.save_state_async();
    }
}

/// Take the resident thread's legacy coucous, stamping `owner` on unscoped ones.
fn drain_legacy(state: &mut cp_base::state::runtime::State, owner: Option<&str>) -> Vec<cp_mod_spine::coucou::Record> {
    let mut list = std::mem::take(&mut SpineState::get_mut(state).legacy_coucous);
    for coucou in &mut list {
        if coucou.thread_id.is_none() {
            coucou.thread_id = owner.map(str::to_owned);
        }
    }
    list
}
