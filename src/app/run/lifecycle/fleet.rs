//! Background-thread advancement — the multi-thread loop step (Phase C).
//!
//! The focused (resident) thread is stepped in place by `run_background_phase`
//! in the parent [`lifecycle`](super) module; this module advances every *other*
//! active thread by swapping its parked [`ThreadRuntime`] into
//! [`State`](crate::state::State) for one step, then swapping the focused thread
//! back. At N=1 the registry is empty, so the whole pass is a no-op and the tick
//! is byte-identical to single-thread execution.

use cp_fleet::{Entry, Role, ThreadExecState, promote};
use cp_mod_threads::types::{ThreadStatus, ThreadsState};

use crate::app::App;

/// Consecutive terminal stream failures (each after its own API retries are
/// exhausted) before a background thread is parked as
/// [`Errored`](ThreadExecState::Errored).
///
/// A one-off blip stays `Runnable` and retries after the spine's exponential
/// backoff; only a thread that keeps failing is declared *stuck* and removed
/// from scheduling until a human re-engages it (design doc §8 — human
/// intervention is the recovery path for a stuck thread). The counter
/// (`SpineState.config.consecutive_continuation_errors`) is reset to 0 by a
/// successful stream completion or a fresh user message.
const STUCK_ERROR_THRESHOLD: usize = 3;

#[expect(clippy::multiple_inherent_impl, reason = "App methods split across run/ submodules for readability")]
impl App {
    /// Reconcile the fleet registry against [`ThreadsState`] — the roster-mirror
    /// that keeps [`fleet`](crate::app::App::fleet) in step with the thread list
    /// each tick, before [`advance_background_threads`](Self::advance_background_threads)
    /// steps the schedulable ones.
    ///
    /// Reconcile rules (the **resident/focused** thread is excluded — its context
    /// lives flat in [`State`](crate::state::State), not in an `Entry`):
    /// - a non-focused, non-archived thread missing from the registry gets a
    ///   fresh `Idle` [`Entry`] (empty [`ThreadRuntime`](cp_base::state::runtime::bundle::ThreadRuntime));
    /// - an entry whose thread vanished, was archived, or became the focused
    ///   resident is removed;
    /// - each surviving **non-active** entry's `exec_state` is derived from its
    ///   thread status (`MyTurn` → `Runnable`, else `Idle`); active entries are
    ///   left to the step loop (see [`derive_exec_state`](Self::derive_exec_state)).
    ///
    /// Promotion + stepping live in `advance_background_threads`; reconcile only
    /// mirrors membership and the Idle↔Runnable derivation.
    pub(super) fn reconcile_fleet_registry(&mut self, now_ms: u64) {
        let focused = cp_mod_threads::types::FocusState::get(&self.state).focused_thread_id.clone();

        // Snapshot the current roster: (id, status, eligible) for every thread
        // that should own a registry entry (non-focused, non-archived).
        let roster: Vec<(String, ThreadStatus)> = ThreadsState::get(&self.state)
            .threads
            .iter()
            .filter(|t| !t.archived && focused.as_deref() != Some(t.id.as_str()))
            .map(|t| (t.id.clone(), t.status))
            .collect();

        // Drop entries whose thread is gone / archived / now the focused resident.
        let live: std::collections::HashSet<&str> = roster.iter().map(|entry| entry.0.as_str()).collect();
        let stale: Vec<String> =
            self.fleet.iter().map(|entry| entry.0.clone()).filter(|id| !live.contains(id.as_str())).collect();
        for id in stale {
            let _removed = self.fleet.remove(&id);
        }

        // Add missing entries, then derive exec_state for each roster thread.
        for entry in &roster {
            let (id, status) = (&entry.0, entry.1);
            if !self.fleet.contains(id) {
                self.fleet.insert(
                    id.clone(),
                    Entry::new(Role::Thread, cp_base::state::runtime::bundle::ThreadRuntime::new()),
                );
            }
            if let Some(reg_entry) = self.fleet.get_mut(id) {
                Self::derive_exec_state(reg_entry, status, now_ms);
            }
        }

        // Promotion is applied by `advance_background_threads` (it needs to
        // STEP the promoted threads, so the decision lives there). Reconcile is
        // purely the roster-mirror + Idle<->Runnable derivation above.
    }

    /// Boot-load every background thread's persisted per-thread context into the
    /// fleet registry — the boot half of F1 (design doc I-10 "boot loads N →
    /// registry"). Called once at the start of [`run`](super::App::run), before
    /// the loop's first [`reconcile_fleet_registry`](Self::reconcile_fleet_registry).
    ///
    /// For each non-focused, non-archived thread it calls
    /// [`boot_load_thread_runtime`](crate::state::persistence::boot_load_thread_runtime),
    /// which returns the thread's persisted [`ThreadRuntime`] or `None` when the
    /// thread has no `states/<tid>.json` yet (a cold thread). A loaded runtime is
    /// registered with its `exec_state` derived from the thread's status; a cold
    /// thread is left for `reconcile_fleet_registry` to insert with a fresh empty
    /// runtime on the first tick.
    ///
    /// At N=1 (and for any agent whose background threads have not yet been
    /// persisted per-thread) every lookup is `None`, so the fleet stays empty and
    /// boot is byte-identical: reconcile then behaves exactly as before.
    pub(super) fn load_background_threads(&mut self) {
        let focused = cp_mod_threads::types::FocusState::get(&self.state).focused_thread_id.clone();
        let now = cp_base::panels::now_ms();
        let roster: Vec<(String, ThreadStatus)> = ThreadsState::get(&self.state)
            .threads
            .iter()
            .filter(|t| !t.archived && focused.as_deref() != Some(t.id.as_str()))
            .map(|t| (t.id.clone(), t.status))
            .collect();

        for (id, status) in roster {
            let Some(runtime) = crate::state::persistence::boot_load_thread_runtime(&id) else {
                continue; // cold thread: no persisted file; reconcile gives it a fresh runtime
            };
            let mut entry = Entry::new(Role::Thread, runtime);
            Self::derive_exec_state(&mut entry, status, now);
            self.fleet.insert(id, entry);
        }
    }

    /// Derive one non-resident entry's [`ThreadExecState`] from its thread status.
    ///
    /// An **active** entry ([`Streaming`](ThreadExecState::Streaming) /
    /// [`AwaitingLlm`](ThreadExecState::AwaitingLlm)) is owned by the advancement
    /// step loop ([`advance_background_threads`](Self::advance_background_threads)
    /// re-derives it from the post-step stream phase), so reconcile leaves it
    /// untouched — otherwise a mid-stream thread, whose `ThreadStatus` is still
    /// `MyTurn`, would be clobbered back to `Runnable` every tick.
    ///
    /// For a non-active entry: `MyTurn` means user input awaits the agent →
    /// `Runnable` (stamping `waiting_since_ms` once, on the Idle→Runnable edge,
    /// for oldest-waiting-first promotion); `TheirTurn` → `Idle` (waiting on the
    /// human), clearing any stale wait stamp.
    fn derive_exec_state(
        entry: &mut Entry<cp_base::state::runtime::bundle::ThreadRuntime>,
        status: ThreadStatus,
        now_ms: u64,
    ) {
        if entry.exec_state.is_active() || entry.exec_state == ThreadExecState::Errored {
            // Active: owned by the advancement step loop (re-derived post-step).
            // Errored: parked as stuck — never auto-revived here; only a fresh
            // user message clears it (see `clear_errored_entry`), matching the
            // human-intervention recovery model. Never clobber either back to
            // Runnable just because the thread's status is still MyTurn.
            return;
        }
        match status {
            ThreadStatus::MyTurn => {
                if entry.exec_state != ThreadExecState::Runnable {
                    entry.exec_state = ThreadExecState::Runnable;
                    entry.waiting_since_ms = Some(now_ms);
                }
            }
            ThreadStatus::TheirTurn => {
                entry.exec_state = ThreadExecState::Idle;
                entry.waiting_since_ms = None;
            }
        }
    }

    /// Advance every schedulable *background* (non-resident, capped) thread one
    /// step: the currently-active ones (continuing their stream) plus the ones
    /// [`promote`] selects to fill free concurrency slots (their kickoff step).
    ///
    /// For each, it removes the entry, marks it the stepping resident, swaps its
    /// parked [`ThreadRuntime`](cp_base::state::runtime::bundle::ThreadRuntime)
    /// into `state`, runs the advancement core
    /// ([`step_one_thread`](Self::step_one_thread)) — whose `check_spine` may
    /// start the thread's stream — then re-derives the entry's
    /// [`ThreadExecState`] from the post-step stream phase, swaps the focused
    /// thread back, and re-inserts. The swap is O(1), so a tick costs at most
    /// `K` swaps. Empty at N=1 (no background peer has work) → no-op.
    pub(super) fn advance_background_threads(&mut self) {
        let focused = cp_mod_threads::types::FocusState::get(&self.state).focused_thread_id.clone();
        let promotable: std::collections::HashSet<String> = promote(&self.fleet).into_iter().collect();
        let ids: Vec<String> = self
            .fleet
            .iter()
            .filter(|entry| {
                entry.1.role.is_capped()
                    && (entry.1.exec_state.is_active() || promotable.contains(entry.0.as_str()))
                    && focused.as_deref() != Some(entry.0.as_str())
            })
            .map(|entry| entry.0.clone())
            .collect();

        for id in ids {
            // Remove the entry so the registry borrow ends before touching
            // `state` — the fleet and state `&mut self` sub-borrows must not
            // overlap. Re-inserted after the step.
            let Some(mut entry) = self.fleet.remove(&id) else { continue };
            // Mark this thread resident so `resident_key` (stream spawn + drain)
            // targets ITS channel during the step, not the focused thread's, and
            // so the stream tee tags this thread's live frames with ITS id.
            self.stepping_thread = Some(id.clone());
            entry.runtime.swap_with(&mut self.state); // thread `id` resident; focused parks into entry
            self.state.resident_thread_id = Some(id.clone());
            self.step_one_thread();
            entry.exec_state = self.post_step_exec_state(&id); // from this thread's new stream phase
            entry.runtime.swap_with(&mut self.state); // restore focused; thread `id` parks back
            self.stepping_thread = None;
            self.state.resident_thread_id.clone_from(&focused);
            self.fleet.insert(id, entry);
        }
    }

    /// Re-derive a just-stepped background thread's [`ThreadExecState`] from the
    /// state it left behind (the thread is still resident in `state`).
    ///
    /// A started/continuing stream → [`Streaming`](ThreadExecState::Streaming)
    /// (holds a concurrency slot). Otherwise, if the thread's shared
    /// [`ThreadStatus`] is still `MyTurn` it has unfinished work →
    /// [`Runnable`](ThreadExecState::Runnable) (eligible for re-promotion); a
    /// `TheirTurn` thread has handed back to the human →
    /// [`Idle`](ThreadExecState::Idle).
    fn post_step_exec_state(&self, id: &str) -> ThreadExecState {
        if self.state.flags.stream.phase.is_streaming() {
            return ThreadExecState::Streaming;
        }
        // Repeated terminal failures → park as Errored (stuck, needs a human).
        // Excluded from both `promote` and the step loop, so a persistently
        // failing thread stops churning a promotion slot and can never stall the
        // fleet; it waits for a fresh user message to re-engage it.
        let errs = cp_mod_spine::types::SpineState::get(&self.state).config.consecutive_continuation_errors;
        if errs >= STUCK_ERROR_THRESHOLD {
            return ThreadExecState::Errored;
        }
        let status = ThreadsState::get(&self.state).threads.iter().find(|t| t.id == id).map(|t| t.status);
        match status {
            Some(ThreadStatus::MyTurn) => ThreadExecState::Runnable,
            _ => ThreadExecState::Idle,
        }
    }

    /// The per-thread advancement core: drain this thread's stream, retry a
    /// failed request, flush the typewriter, run its tool pipeline, finalize a
    /// completed stream, and evaluate its spine. Called once per background
    /// thread while it is swapped in. Fleet-global work (bridge, cache wait,
    /// reverie) is NOT here — it runs once per tick, not once per thread.
    fn step_one_thread(&mut self) {
        super::super::streaming::process_stream_events(self);
        super::super::streaming::handle_retry(self);
        super::super::streaming::process_typewriter(self);
        super::super::tools::pipeline::handle_tool_execution(self);
        super::super::streaming::finalize_stream(self);
        self.check_spine();
    }

    /// Nudge every **background** `MyTurn` thread that is idle and has no live
    /// notification, so [`check_spine`](Self::check_spine) continues it once it
    /// is promoted and stepped (the per-thread *dispatcher*, design doc §7.3).
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
                if state.flags.stream.phase.is_streaming() {
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
        // Background owner: swap its parked runtime in, deliver, swap back.
        let Some(mut entry) = self.fleet.remove(tid) else {
            log::warn!("deliver_to_thread: unknown/unparked thread {tid}; delivering to resident");
            f(&mut self.state);
            return;
        };
        entry.runtime.swap_with(&mut self.state); // thread `tid` resident; focused parks into entry
        f(&mut self.state);
        entry.runtime.swap_with(&mut self.state); // restore focused; thread `tid` parks back
        self.fleet.insert(tid.to_owned(), entry);
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
}
