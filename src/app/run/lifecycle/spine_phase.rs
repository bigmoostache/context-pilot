//! Spine step of the main loop: focused-thread spine check, API-check results,
//! then one advancement round of the background threads. Each call carries its
//! own perf span so `loop.spine` self time is attributable.

use crate::app::App;

#[expect(clippy::multiple_inherent_impl, reason = "App methods split across run/ submodules for readability")]
impl App {
    /// Run the `Step::Spine` body for this tick.
    pub(super) fn run_spine_phase(&mut self, current_ms: u64) {
        {
            let _g = crate::profile!("sp_check_spine");
            self.check_spine();
        }
        {
            let _g = crate::profile!("sp_api_check_results");
            super::super::streaming::process_api_check_results(self);
        }

        // === BACKGROUND THREADS (Phase C) ===
        // After the focused thread has been stepped, advance every OTHER active
        // thread one step by making it executing around the same advancement
        // core (`step_one_thread`). No-op at N=1. Reconcile first: registry vs
        // roster + promotion decision.
        {
            let _g = crate::profile!("sp_reconcile_fleet");
            self.reconcile_fleet_registry(current_ms);
        }
        {
            let _g = crate::profile!("sp_dispatch_bg");
            self.dispatch_background_my_turn();
        }
        {
            let _g = crate::profile!("sp_advance_bg");
            self.advance_background_threads();
        }
        // G2 display mirror: republish AFTER the step loop so it reflects
        // post-step derivations; the loop is at rest, so `state` is the focused
        // thread again.
        {
            let _g = crate::profile!("sp_publish_fleet_view");
            self.publish_fleet_view_states();
        }
    }
}
