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
        if entry.exec_state.is_active() {
            return; // owned by the advancement step loop; never clobber
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
}
