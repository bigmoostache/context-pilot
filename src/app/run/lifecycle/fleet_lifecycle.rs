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

use crate::app::App;

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

        // Union in each background thread's keys via a transient swap (pure read;
        // no stream is spawned, so no `stepping_thread`/`resident_thread_id`
        // bookkeeping is needed — those only matter for stream spawn/drain).
        let bg_ids: Vec<String> = self.fleet.iter().map(|entry| entry.0.clone()).collect();
        for id in bg_ids {
            let Some(mut entry) = self.fleet.remove(&id) else { continue };
            entry.runtime.swap_with(&mut self.state); // thread `id` resident
            known.extend(ConsoleState::get(&self.state).sessions.keys().cloned());
            entry.runtime.swap_with(&mut self.state); // restore focused
            self.fleet.insert(id, entry);
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
        self.state.resident_thread_id.clone_from(&focused);
        known.extend(save::panel_uids_of(&self.state));
        save::save_state(&self.state);

        // Each background thread: swap in, snapshot to its own file, swap back.
        let bg_ids: Vec<String> = self.fleet.iter().map(|entry| entry.0.clone()).collect();
        for id in bg_ids {
            let Some(mut entry) = self.fleet.remove(&id) else { continue };
            self.stepping_thread = Some(id.clone());
            entry.runtime.swap_with(&mut self.state); // thread `id` resident; focused parks into entry
            self.state.resident_thread_id = Some(id.clone());
            known.extend(save::panel_uids_of(&self.state));
            save::save_state(&self.state); // → states/<id>.json (resident != focused), no deletes
            entry.runtime.swap_with(&mut self.state); // restore focused; thread `id` parks back
            self.stepping_thread = None;
            self.fleet.insert(id, entry);
        }
        self.state.resident_thread_id = focused;

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
    }

    /// Render-scoped drill-in: temporarily make the human-drilled thread
    /// ([`FocusState::drilled_thread_id`](cp_mod_threads::types::FocusState)) resident
    /// in `state` **only for the duration of one paint**, returning the removed
    /// registry entry the caller must pass to
    /// [`restore_drilled_runtime_after_render`](Self::restore_drilled_runtime_after_render)
    /// right after `terminal.draw` to swap it back.
    ///
    /// This is the G3 "pixel-identical panel view" mechanism and the one place
    /// focus-as-view and execution are deliberately decoupled (Model 2): the
    /// drilled thread's panels are painted by swapping its parked
    /// [`ThreadRuntime`](cp_base::state::runtime::bundle::ThreadRuntime) into
    /// `state`, then restored before the next tick — so `resident_thread_id`,
    /// `focused_thread_id`, and all scheduling are untouched and a human glance
    /// never parks a mid-stream thread.
    ///
    /// Returns `None` (no swap) when there is no drill-in, when the drilled
    /// thread **is** the resident (already flat in `state` — painted directly),
    /// or when it has no registry entry (cold/unknown). At N=1 the field is
    /// always `None`, so this is a no-op and rendering is byte-identical.
    pub(super) fn take_drilled_runtime_for_render(
        &mut self,
    ) -> Option<(String, cp_fleet::Entry<cp_base::state::runtime::bundle::ThreadRuntime>)> {
        let drilled = cp_mod_threads::types::FocusState::get(&self.state).drilled_thread_id.clone()?;
        // Resident (== focused) thread is already flat in `state`; nothing to swap.
        if self.state.resident_thread_id.as_deref() == Some(drilled.as_str()) {
            return None;
        }
        let mut entry = self.fleet.remove(&drilled)?;
        entry.runtime.swap_with(&mut self.state); // drilled thread resident for the paint; focused parks into entry
        Some((drilled, entry))
    }

    /// Undo [`take_drilled_runtime_for_render`](Self::take_drilled_runtime_for_render):
    /// swap the focused thread back into `state` and re-park the drilled thread's
    /// runtime in the registry, leaving `state` + fleet exactly as before the paint.
    pub(super) fn restore_drilled_runtime_after_render(
        &mut self,
        drilled: Option<(String, cp_fleet::Entry<cp_base::state::runtime::bundle::ThreadRuntime>)>,
    ) {
        if let Some((id, mut entry)) = drilled {
            entry.runtime.swap_with(&mut self.state); // restore focused resident; drilled parks back
            self.fleet.insert(id, entry);
        }
    }
}
