//! Background-thread advancement — the multi-thread loop step (Phase C).
//!
//! The focused (resident) thread is stepped in place by `run_background_phase`
//! in the parent [`lifecycle`](super) module; this module advances every *other*
//! active thread by swapping its parked [`ThreadRuntime`] into
//! [`State`](crate::state::State) for one step, then swapping the focused thread
//! back. At N=1 the registry is empty, so the whole pass is a no-op and the tick
//! is byte-identical to single-thread execution.

use crate::app::App;

#[expect(clippy::multiple_inherent_impl, reason = "App methods split across run/ submodules for readability")]
impl App {
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
}
