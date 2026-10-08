//! Fleet lifecycle I/O — the boot/persist/teardown half of the multi-thread
//! model (Phase F), split from the sibling [`fleet`](super::fleet) scheduling
//! module to keep each file under the 500-line cap.
//!
//! `fleet` owns the per-tick *scheduling* (reconcile, promote, advance, the
//! [`deliver_to_thread`](crate::app::App::deliver_to_thread) delivery seam);
//! this file owns the *lifecycle* operations that touch disk and external
//! resources: the fleet-wide console orphan-prune (F6), the N-thread save
//! (F1), per-thread hard-delete teardown (F5), and the human-intervention
//! re-engage of an `Errored` thread (F4).

use cp_fleet::ThreadExecState;
use cp_mod_threads::types::{ThreadStatus, ThreadsState};

use crate::app::App;

/// Executing thread to restore after a drill-in paint (`None` = unbound).
pub(super) struct DrillRestore {
    /// Thread that was executing before the drilled thread was made current.
    prev: Option<String>,
}

#[expect(clippy::multiple_inherent_impl, reason = "App methods split across run/ submodules for readability")]
impl App {
    /// Kill console-server sessions that belong to **no** loaded thread, in a
    /// single fleet-wide pass over the union of every thread's session keys
    /// (design doc §F6 / S2 — the reattach-by-thread half of lifecycle).
    ///
    /// Why this is not in the console module's `load_module_data`: that hook runs
    /// once *per thread* at boot, but the console server's session map is shared
    /// fleet-wide. Killing orphans relative to one thread's keys would kill every
    /// *other* thread's live sessions (the exact class of bug as the F1c
    /// per-thread panel prune). So the per-thread kill was removed and this pass
    /// runs it once, after [`load_background_threads`](Self::load_background_threads),
    /// over the union of the focused thread's keys plus each parked thread's keys.
    ///
    /// Session reconnection stays per-thread (each thread reattaches its own
    /// sessions when its module data loads); only the orphan *kill* is unioned.
    ///
    /// N=1-identical: the fleet is empty, so the union is just the focused
    /// thread's keys — exactly what the removed `load_module_data` kill used
    /// (and an empty set kills every orphan, matching the old empty-sessions
    /// branch).
    pub(super) fn prune_orphaned_console_sessions(&mut self) {
        use cp_mod_console::types::ConsoleState;

        // Focused (resident) thread's live session keys.
        let mut known: std::collections::HashSet<String> =
            ConsoleState::get(&self.state).sessions.keys().cloned().collect();

        // Union in each background thread's keys (pure read, no stream spawned).
        let bg_ids: Vec<String> = self.fleet.iter().map(|entry| entry.0.clone()).collect();
        for id in bg_ids {
            self.deliver_to_thread(Some(&id), |state| {
                known.extend(ConsoleState::get(state).sessions.keys().cloned());
            });
        }

        cp_mod_console::manager::kill_orphaned_processes(&known);
    }

    /// Persist **every** thread to disk — the resident (focused) thread first,
    /// then each background thread by swapping its parked
    /// [`ThreadRuntime`](cp_base::state::runtime::bundle::ThreadRuntime) into
    /// `state`, saving, and swapping back (design doc §F1 "save stores the
    /// resident first", I-10). Called on reload and quit, replacing the
    /// single-thread `save_state`.
    ///
    /// Why this exists and not just `save_state`: `build_save_batch` deliberately
    /// no longer prunes orphaned `panels/<uid>.json` per call (that would delete
    /// *other* threads' panels — the dir is shared, keyed by the fleet-global UID
    /// counter). So each thread is saved with empty deletes here, their live
    /// panel UIDs are unioned via [`panel_uids_of`](crate::state::persistence::save::panel_uids_of),
    /// and the orphan-prune runs **exactly once** over that union at the end.
    ///
    /// The swap machinery mirrors
    /// [`advance_background_threads`](Self::advance_background_threads): mark the
    /// stepping thread so `resident_worker_id` routes its save to
    /// `states/<tid>.json`, swap in, save, swap the focused thread back. At N=1
    /// the fleet is empty, so this saves only the focused thread to
    /// `main_worker.json` and prunes over its UIDs — byte-identical to the former
    /// `save_state` (bar the deferred-prune timing, which is inert: boot loads
    /// from the persisted UID index, never a dir scan).
    pub(super) fn save_all_threads(&mut self) {
        use crate::state::persistence::save;

        let focused = cp_mod_threads::types::FocusState::get(&self.state).focused_thread_id.clone();
        let mut known: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Resident (focused) first → states/main_worker.json (focused == resident).
        self.state.thread_store.set_executing(focused.clone());
        known.extend(save::panel_uids_of(&self.state));
        save::save_state(&self.state);

        // Each background thread: execute it, snapshot to its own file.
        let bg_ids: Vec<String> = self.fleet.iter().map(|entry| entry.0.clone()).collect();
        for id in bg_ids {
            if !self.state.thread_store.contains(&id) {
                continue;
            }
            self.state.thread_store.set_executing(Some(id));
            known.extend(save::panel_uids_of(&self.state));
            save::save_state(&self.state); // → states/<id>.json (executing != focused), no deletes
        }
        self.state.thread_store.set_executing(focused);

        // Union orphan-prune, exactly once over every thread's live UIDs.
        for del in save::collect_orphan_deletes(&save::panels_dir(), &known) {
            save::exec_delete_op(&del);
        }
    }

    /// Re-engage a background thread parked as [`Errored`](ThreadExecState::Errored),
    /// flipping its registry entry back to `Runnable` so the loop schedules it again.
    ///
    /// This is the **human-intervention recovery path** (design doc §8): a stuck
    /// thread is only revived by a fresh user message, which `route_on_user_message`
    /// routes here after resetting the thread's spine error counters. A no-op when
    /// the thread has no entry (the focused resident, or an unknown thread) or when
    /// the entry is not `Errored` — so at N=1 (focused thread never in the registry)
    /// it never fires.
    pub(crate) fn clear_errored_entry(&mut self, thread_id: &str) {
        if let Some(entry) = self.fleet.get_mut(thread_id)
            && entry.exec_state == ThreadExecState::Errored
        {
            entry.exec_state = ThreadExecState::Runnable;
            entry.waiting_since_ms = Some(cp_base::panels::now_ms());
        }
    }

    /// Structural teardown of a **hard-deleted** thread (design doc §F5 / H15):
    /// kill its external console processes, then drop its runtime bundle (which
    /// drops the thread's per-thread resources — the [`WatcherRegistry`], queue,
    /// spine inbox — cancelling its in-process watchers by `Drop`).
    ///
    /// Called from `apply_command`'s `DeleteThread` arm **after**
    /// [`apply_delete_thread`](super::super::threads::commands) has removed the
    /// thread from [`ThreadsState`] and cleared focus/bridge memos. The thread id
    /// is the single teardown key.
    ///
    /// Console sessions are per-thread ([`ConsoleState`](cp_mod_console::types::ConsoleState)
    /// is `is_global() == false`), so
    /// [`deliver_to_thread`](Self::deliver_to_thread) swaps the thread's context
    /// in and [`shutdown_all`](cp_mod_console::types::ConsoleState::shutdown_all)
    /// kills exactly *its* sessions (by their own keys, via the existing
    /// per-session kill — no server protocol change), then swaps the focused
    /// thread back. The in-process watcher drop is `Drop` on the removed
    /// [`ThreadRuntime`](cp_base::state::runtime::bundle::ThreadRuntime): a
    /// [`ChannelWatcher`](cp_base::state::watchers::ChannelWatcher) drops its
    /// receiver so its worker thread's later send fails harmlessly (cancellation);
    /// timer/console watchers are data-only.
    ///
    /// N=1 identical: deleting the focused resident runs `shutdown_all` directly
    /// on `state` and [`fleet.remove`](cp_fleet::FleetRegistry::remove) is a no-op
    /// (the focused thread is never in the registry); deleting a background thread
    /// takes the swap path. Either way behaviour matches single-thread, and no
    /// console key or on-disk path changes.
    pub(crate) fn teardown_thread(&mut self, thread_id: &str) {
        self.deliver_to_thread(Some(thread_id), |state| {
            cp_mod_console::types::ConsoleState::shutdown_all(state);
        });
        let _removed = self.fleet.remove(thread_id);
        drop(self.state.thread_store.remove(thread_id));
        let _parked = self.parked_stream_runtimes.remove(thread_id);
        // Deleting the executing thread: fall back to an initialized unbound
        // runtime so the next render's per-thread `ext` lookups still resolve.
        if self.state.executing_thread_id() == Some(thread_id) {
            let fresh = self.fresh_runtime();
            drop(self.state.thread_store.take_unbound(fresh));
            self.state.thread_store.set_executing(None);
        }
    }

    /// Render-scoped drill-in: make the human-drilled thread
    /// ([`FocusState::drilled_thread_id`](cp_mod_threads::types::FocusState)) the
    /// executing one **for one paint only**. Returns the previous executing id,
    /// which the caller passes to
    /// [`restore_drilled_runtime_after_render`](Self::restore_drilled_runtime_after_render)
    /// right after `terminal.draw`.
    ///
    /// Focus-as-view and execution stay decoupled (Model 2): no data moves,
    /// `focused_thread_id` and scheduling are untouched, and a human glance never
    /// parks a mid-stream thread.
    ///
    /// `None` when there is no drill-in, the drilled thread is already executing,
    /// or it has no stored runtime (cold/unknown).
    pub(super) fn take_drilled_runtime_for_render(&mut self) -> Option<DrillRestore> {
        let drilled = cp_mod_threads::types::FocusState::get(&self.state).drilled_thread_id.clone()?;
        if self.state.executing_thread_id() == Some(drilled.as_str()) || !self.state.thread_store.contains(&drilled) {
            return None;
        }
        let prev = self.state.executing_thread_id().map(str::to_owned);
        self.state.thread_store.set_executing(Some(drilled));
        Some(DrillRestore { prev })
    }

    /// Undo [`take_drilled_runtime_for_render`](Self::take_drilled_runtime_for_render):
    /// restore the executing thread that was current before the paint.
    pub(super) fn restore_drilled_runtime_after_render(&mut self, restore: Option<DrillRestore>) {
        if let Some(r) = restore {
            self.state.thread_store.set_executing(r.prev);
        }
    }

    /// Nudge every **background** `MyTurn` thread that is idle and has no live
    /// notification, so [`check_spine`](crate::app::App::check_spine) continues
    /// it once it is promoted and stepped (the per-thread *dispatcher*, design
    /// doc §7.3).
    ///
    /// The **focused** thread is deliberately excluded — it is handled by the
    /// [`IdleMyTurnDetector`](cp_mod_threads::watcher::IdleMyTurnDetector) watcher
    /// (focused-only since this dispatcher subsumed its background fallback), so
    /// the two nudge paths never overlap on the same thread.
    ///
    /// For each candidate it swaps the thread in (via
    /// [`deliver_to_thread`](Self::deliver_to_thread)) and, **only if** that
    /// thread is idle (not streaming) and its inbox holds no unprocessed
    /// notification, drops a single `Custom`/`threads` notification bound to it.
    /// Both guards make this flood-safe by construction: a thread mid-stream or
    /// already-nudged is skipped, and [`SpineState::create_notification`] itself
    /// dedups on `(kind, source)`, so re-running every tick never piles up.
    ///
    /// At N=1 the only `MyTurn` thread is the focused resident (excluded), so the
    /// candidate set is empty and this is a no-op — byte-identical to
    /// single-thread.
    pub(super) fn dispatch_background_my_turn(&mut self) {
        let focused = cp_mod_threads::types::FocusState::get(&self.state).focused_thread_id.clone();
        // Snapshot (id, name) of eligible background threads before touching the
        // per-thread state (the swap below borrows `state`).
        let candidates: Vec<(String, String)> = ThreadsState::get(&self.state)
            .threads
            .iter()
            .filter(|t| {
                !t.archived
                    && !t.paused
                    && t.status == ThreadStatus::MyTurn
                    && focused.as_deref() != Some(t.id.as_str())
            })
            .map(|t| (t.id.clone(), t.name.clone()))
            .collect();

        for (tid, name) in candidates {
            let tid_for_bind = tid.clone();
            self.deliver_to_thread(Some(&tid), move |state| {
                // Skip if this thread is already working or already nudged — the
                // two guards that keep the dispatcher from flooding an inbox.
                if state.thread().stream.phase.is_streaming() {
                    return;
                }
                if cp_mod_spine::types::SpineState::has_unprocessed_notifications(state) {
                    return;
                }
                let content = format!(
                    "Thread \"{name}\" ({tid_for_bind}) is MY_TURN and needs a response. \
                     Use Read to focus on it, then Send your reply.",
                );
                let nid = cp_mod_spine::types::SpineState::create_notification(
                    state,
                    cp_mod_spine::types::NotificationType::Custom,
                    "threads".to_owned(),
                    content,
                );
                cp_mod_spine::types::SpineState::set_notification_thread(state, &nid, Some(tid_for_bind));
            });
        }
    }

    /// Run `f` against the state of the thread that owns `thread_id` — the
    /// **delivery seam** for thread-addressed spine routing (Phase D1).
    ///
    /// Delivery is distinct from advancement: it mutates a thread's own context
    /// (its spine inbox, its conversation) without stepping its pipeline. This is
    /// how a watcher fire, a coucou, or a bridge message for a *background*
    /// thread lands in *that* thread's inbox rather than the resident's.
    ///
    /// Routing (the resident = the focused thread, whose context lives flat in
    /// [`State`](cp_base::state::runtime::State)):
    /// - `None`, or a `thread_id` equal to the focused resident → run `f` on
    ///   `state` directly (today's single-thread path);
    /// - any other (background) owner → swap its parked
    ///   [`ThreadRuntime`](cp_base::state::runtime::bundle::ThreadRuntime) into
    ///   `state` (O(1), no clone), run `f`, then swap the focused thread back —
    ///   so `state` and the registry are left exactly as found.
    ///
    /// At N=1 the only working thread is the focused resident, so every target
    /// is `None` or the resident and the swap branch is never taken: behaviour
    /// is byte-identical to single-thread. A `thread_id` that is neither focused
    /// nor in the registry (unknown/archived) is logged and delivered to the
    /// resident as a last-resort safety net (never reached at N=1).
    pub(crate) fn deliver_to_thread<F>(&mut self, thread_id: Option<&str>, f: F)
    where
        F: FnOnce(&mut cp_base::state::runtime::State),
    {
        let focused = cp_mod_threads::types::FocusState::get(&self.state).focused_thread_id.clone();
        let Some(tid) = thread_id.filter(|t| focused.as_deref() != Some(*t)) else {
            // None, or targets the focused resident → deliver directly.
            f(&mut self.state);
            return;
        };
        if !self.state.thread_store.contains(tid) {
            log::warn!("deliver_to_thread: unknown thread {tid}; delivering to the executing thread");
            f(&mut self.state);
            return;
        }
        // Background owner: execute it for the duration of `f`, then restore.
        let prev = self.state.executing_thread_id().map(str::to_owned);
        self.state.thread_store.set_executing(Some(tid.to_owned()));
        f(&mut self.state);
        self.state.thread_store.set_executing(prev);
    }
}
