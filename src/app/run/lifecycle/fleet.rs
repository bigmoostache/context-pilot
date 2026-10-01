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
    /// Reconcile the fleet registry against [`ThreadsState`] and compute the
    /// promotion decision — the **scheduling-decision layer** (Phase C4).
    ///
    /// This mirrors the thread roster into [`fleet`](crate::app::App::fleet) and
    /// derives each entry's [`ThreadExecState`], but deliberately drives **no**
    /// background advancement: it never sets an *active* state
    /// ([`Streaming`](ThreadExecState::Streaming) /
    /// [`AwaitingLlm`](ThreadExecState::AwaitingLlm)), so
    /// [`advance_background_threads`](Self::advance_background_threads) stays a
    /// no-op. Real advancement (flipping a promoted thread active + spawning its
    /// stream) is deferred until the spine is thread-routed (Phase D) and the
    /// conversation is per-thread (Phase F2).
    ///
    /// Reconcile rules (the **resident/focused** thread is excluded — its context
    /// lives flat in [`State`](crate::state::State), not in an `Entry`):
    /// - a non-focused, non-archived thread missing from the registry gets a
    ///   fresh `Idle` [`Entry`] (empty [`ThreadRuntime`](cp_base::state::runtime::bundle::ThreadRuntime));
    /// - an entry whose thread vanished, was archived, or became the focused
    ///   resident is removed;
    /// - each surviving entry's `exec_state` is derived from its thread status:
    ///   `MyTurn` → `Runnable` (+ `waiting_since_ms`), otherwise `Idle`.
    ///
    /// At N=1 the only working thread is the focused resident (excluded), so the
    /// registry holds at most `Idle` `THEIR_TURN` peers, `promote` returns empty,
    /// and the tick is byte-identical to single-thread.
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

        // Promotion decision (fills free active slots from the waiting queue).
        // Wired and unit-tested in cp-fleet; at N=1 the waiting queue is empty
        // (background peers are THEIR_TURN → Idle), so this is provably empty.
        // Applying it (flip → active + spawn stream) is the one step deferred to
        // Phase D/F2 — until then the decision is observed, not executed.
        let promotable = promote(&self.fleet);
        if !promotable.is_empty() {
            log::debug!(
                "fleet: {} thread(s) promotable, advancement deferred to Phase D/F2: {promotable:?}",
                promotable.len()
            );
        }
    }

    /// Derive one non-resident entry's [`ThreadExecState`] from its thread status,
    /// **without ever setting an active state** (advancement stays deferred).
    ///
    /// `MyTurn` means the thread has user input awaiting the agent → `Runnable`
    /// (stamping `waiting_since_ms` once, on the Idle→Runnable edge, for
    /// oldest-waiting-first promotion). `TheirTurn` → `Idle` (waiting on the
    /// human), clearing any stale wait stamp.
    fn derive_exec_state(
        entry: &mut Entry<cp_base::state::runtime::bundle::ThreadRuntime>,
        status: ThreadStatus,
        now_ms: u64,
    ) {
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

    /// Advance every *background* (non-resident, active, capped) thread one step.
    ///
    /// For each one it removes the entry, swaps the thread's parked
    /// [`ThreadRuntime`](cp_base::state::runtime::bundle::ThreadRuntime) into
    /// `state` (making it momentarily resident), runs the advancement core
    /// ([`step_one_thread`](Self::step_one_thread)), then swaps the focused
    /// thread back and re-inserts the entry — so `state` and the registry are
    /// left exactly as they were found. The swap is O(1) (no clone), so a tick
    /// costs at most `K-1` swaps.
    ///
    /// Only `Streaming` / `AwaitingLlm` peer threads are stepped (they hold a
    /// concurrency slot). `Runnable` ones wait for promotion (C4); reveries are
    /// driven by the separate reverie block. Empty at N=1 → no-op.
    pub(super) fn advance_background_threads(&mut self) {
        let focused = cp_mod_threads::types::FocusState::get(&self.state).focused_thread_id.clone();
        let ids: Vec<String> = self
            .fleet
            .iter()
            .filter(|entry| {
                entry.1.role.is_capped()
                    && entry.1.exec_state.is_active()
                    && focused.as_deref() != Some(entry.0.as_str())
            })
            .map(|entry| entry.0.clone())
            .collect();

        for id in ids {
            // Remove the entry so the registry borrow ends before touching
            // `state` — the fleet and state `&mut self` sub-borrows must not
            // overlap. Re-inserted after the step.
            let Some(mut entry) = self.fleet.remove(&id) else { continue };
            entry.runtime.swap_with(&mut self.state); // thread `id` resident; focused parks into entry
            self.step_one_thread();
            entry.runtime.swap_with(&mut self.state); // restore focused; thread `id` parks back
            self.fleet.insert(id, entry);
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
