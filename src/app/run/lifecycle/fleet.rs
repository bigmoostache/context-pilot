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

use cp_base::state::runtime::bundle::ThreadRuntime;

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
    /// Make the **focused** thread the executing one when focus changes.
    ///
    /// Every thread's runtime lives permanently in
    /// [`ThreadStore`](cp_base::state::runtime::threads::ThreadStore); switching
    /// is an id change, no context moves. What still travels is the per-stream
    /// [`StreamRuntime`](super::stream_runtime::StreamRuntime) on `App`
    /// (removed in step 5).
    ///
    /// MUST run before `reconcile_fleet_registry` and before the focused
    /// pipeline steps, so both see the new focus as executing.
    ///
    /// - `want == have`: no-op.
    /// - outgoing `have`: its runtime stays in its slot. If it is mid-execution
    ///   (live stream, pending console-wait, pending tools, deferred
    ///   `StreamDone`) its fleet entry is marked `Streaming`, so the step loop
    ///   keeps advancing it after it loses focus (X805).
    /// - incoming `want` with no slot: on first placement (`have` is `None`) it
    ///   adopts the boot-assembled unbound runtime; otherwise (cold thread) it
    ///   gets a freshly initialized one.
    /// - `want` is `None` (focus cleared): the unbound runtime becomes current,
    ///   re-initialized so per-thread module states exist for the next render.
    pub(super) fn relocate_resident_on_focus_change(&mut self) {
        let want = cp_mod_threads::types::FocusState::get(&self.state).focused_thread_id.clone();
        let have = self.state.executing_thread_id().map(str::to_owned);
        if want == have {
            return;
        }

        // A deleted outgoing thread (slot gone) has nothing to park.
        if let Some(have_id) = have.as_deref().filter(|id| self.state.thread_store.contains(id)) {
            self.park_outgoing(have_id.to_owned());
        }
        self.place_incoming(want.as_deref(), have.is_none());
        self.state.thread_store.set_executing(want);
    }

    /// Park the outgoing thread's per-stream runtime (so its typewriter and
    /// pending tools travel with it) and register it for background stepping.
    /// A mid-exec thread is marked `Streaming`, not `Idle`: its `ThreadStatus`
    /// is `TheirTurn` between turns, so `Idle` would drop it from scheduling.
    /// `post_step_exec_state` re-derives the real state on its first step.
    fn park_outgoing(&mut self, have_id: String) {
        let _g = crate::profile!("rr_park");
        let mut sr = super::stream_runtime::StreamRuntime::new();
        sr.swap_with_app(self);
        let mid_exec = self.state.stream.phase.is_streaming()
            || sr.pending_console_wait_tool_results.is_some()
            || !sr.pending_tools.is_empty()
            || sr.pending_done.is_some();
        let _prev = self.parked_stream_runtimes.insert(have_id.clone(), sr);
        let role = self.fleet.get(&have_id).map_or(Role::Thread, |e| e.role);
        let mut entry = Entry::new(role);
        if mid_exec {
            entry.exec_state = ThreadExecState::Streaming;
        }
        self.fleet.insert(have_id, entry);
    }

    /// Give the incoming thread a stored runtime if it has none (the boot
    /// unbound runtime on `first_placement`, else a fresh one), take it out of
    /// the background registry and restore its per-stream runtime. `None`
    /// (focus cleared) re-initializes the unbound runtime instead.
    fn place_incoming(&mut self, want: Option<&str>, first_placement: bool) {
        let Some(want_id) = want else {
            let fresh = self.fresh_runtime();
            drop(self.state.thread_store.take_unbound(fresh));
            return;
        };
        if !self.state.thread_store.contains(want_id) {
            let runtime = if first_placement {
                self.state.thread_store.take_unbound(ThreadRuntime::new())
            } else {
                self.fresh_runtime()
            };
            self.state.thread_store.insert(want_id.to_owned(), runtime);
        }
        let _removed = self.fleet.remove(want_id);
        if let Some(mut sr) = self.parked_stream_runtimes.remove(want_id) {
            let _g = crate::profile!("rr_swap_stream");
            sr.swap_with_app(self);
        }
    }

    /// A freshly initialized runtime (per-thread module states + fixed base
    /// panels), minting panel UIDs from the shared counter.
    pub(super) fn fresh_runtime(&mut self) -> ThreadRuntime {
        let active = self.state.active_modules.clone();
        crate::state::persistence::fresh_thread_runtime(&mut self.state.global_next_uid, &active)
    }

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

        // Drop entries whose thread is gone / archived / now focused. A gone or
        // archived thread also loses its stored runtime; the focused one keeps it.
        let live: std::collections::HashSet<&str> = roster.iter().map(|entry| entry.0.as_str()).collect();
        let stale: Vec<String> =
            self.fleet.iter().map(|entry| entry.0.clone()).filter(|id| !live.contains(id.as_str())).collect();
        for id in stale {
            let _removed = self.fleet.remove(&id);
            if focused.as_deref() != Some(id.as_str()) {
                drop(self.state.thread_store.remove(&id));
            }
        }

        // Add missing entries, then derive exec_state for each roster thread.
        for entry in &roster {
            let (id, status) = (&entry.0, entry.1);
            if !self.state.thread_store.contains(id) {
                // A cold thread (in the roster, no persisted file) still needs a
                // runtime whose per-thread module states are INITIALIZED: a bare
                // `ThreadRuntime::new()` has an empty module map, so its first
                // step's `ext::<SpineState>()` would panic.
                let runtime = self.fresh_runtime();
                self.state.thread_store.insert(id.clone(), runtime);
            }
            if !self.fleet.contains(id) {
                self.fleet.insert(id.clone(), Entry::new(Role::Thread));
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

        let active = self.state.active_modules.clone();
        for (id, status) in roster {
            let Some(runtime) =
                crate::state::persistence::boot_load_thread_runtime(&id, &mut self.state.global_next_uid, &active)
            else {
                continue; // cold thread: no persisted file; reconcile gives it a fresh runtime
            };
            self.state.thread_store.insert(id.clone(), runtime);
            let mut entry = Entry::new(Role::Thread);
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
    fn derive_exec_state(entry: &mut Entry, status: ThreadStatus, now_ms: u64) {
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

        let prev = self.state.executing_thread_id().map(str::to_owned);
        for id in ids {
            if !self.fleet.contains(&id) || !self.state.thread_store.contains(&id) {
                continue;
            }
            // Execute this thread: its stream spawn/drain, tee tag and every
            // per-thread write now target ITS runtime. No data moves.
            self.stepping_thread = Some(id.clone());
            self.state.thread_store.set_executing(Some(id.clone()));
            // Swap this thread's per-stream runtime (typewriter, pending tools/
            // done, console-wait + blocking accumulators, deferred-sleep flags)
            // into `App` so the shared advancement core drains ITS buffers, not
            // the focused thread's — without this, the focused thread's residual
            // typewriter chars / pending tools bleed into this thread (N>1 bug).
            let mut sr = self.parked_stream_runtimes.remove(&id).unwrap_or_default();
            sr.swap_with_app(self);
            self.step_one_thread();
            let exec_state = self.post_step_exec_state(&id); // from this thread's new stream phase
            sr.swap_with_app(self); // restore focused thread's per-stream runtime
            let _prev = self.parked_stream_runtimes.insert(id.clone(), sr);
            self.stepping_thread = None;
            self.state.thread_store.set_executing(prev.clone());
            if let Some(entry) = self.fleet.get_mut(&id) {
                entry.exec_state = exec_state;
            }
        }
    }

    /// Re-derive a just-stepped background thread's [`ThreadExecState`] from the
    /// state it left behind (the thread is still resident in `state`).
    ///
    /// Delegates to [`exec_state_from_residency`](Self::exec_state_from_residency)
    /// so the step loop and the display mirror cannot drift apart.
    fn post_step_exec_state(&self, id: &str) -> ThreadExecState {
        let errs = cp_mod_spine::types::SpineState::get(&self.state).config.consecutive_continuation_errors;
        let status = ThreadsState::get(&self.state).threads.iter().find(|t| t.id == id).map(|t| t.status);
        Self::exec_state_from_residency(self.state.stream.phase.is_streaming(), errs, status)
    }

    /// The single definition of "what exec state do these residency facts imply".
    ///
    /// Shared by two callers that must agree: the step loop's post-step
    /// derivation ([`post_step_exec_state`](Self::post_step_exec_state)) and the
    /// display mirror ([`publish_fleet_view_states`](Self::publish_fleet_view_states)).
    /// A second, drifting definition would show the human a state the scheduler
    /// never actually held.
    ///
    /// Order matters: a live stream wins (the thread holds its `K` slot), then a
    /// fatal error streak parks it as `Errored`, and only then does the turn
    /// status decide `Runnable` vs `Idle`.
    const fn exec_state_from_residency(
        is_streaming: bool,
        consecutive_errors: usize,
        status: Option<ThreadStatus>,
    ) -> ThreadExecState {
        if is_streaming {
            return ThreadExecState::Streaming;
        }
        // Repeated terminal failures → park as Errored (stuck, needs a human).
        // Excluded from both `promote` and the step loop, so a persistently
        // failing thread stops churning a promotion slot and can never stall the
        // fleet; it waits for a fresh user message to re-engage it.
        if consecutive_errors >= STUCK_ERROR_THRESHOLD {
            return ThreadExecState::Errored;
        }
        match status {
            Some(ThreadStatus::MyTurn) => ThreadExecState::Runnable,
            _ => ThreadExecState::Idle,
        }
    }

    /// Republish the display-only exec-state mirror
    /// ([`FleetExecMirror`](cp_mod_threads::view_state::FleetExecMirror)) from the
    /// registry plus the resident's own facts.
    ///
    /// Called once per tick, **after** the step loop, so the map reflects
    /// post-step derivations rather than the pre-step reconcile. Rebuilding
    /// wholesale (not merging) is what lets a deleted or archived thread vanish
    /// from the UI instead of lingering.
    ///
    /// The **focused** thread needs its own source: the registry deliberately
    /// excludes it (its context lives flat in `state`, the resident=focused
    /// invariant), so a registry-only mirror would leave the row the human is
    /// looking at blank. Its stream phase and error streak are read from
    /// `state`, which *is* that thread while the loop is at rest.
    pub(super) fn publish_fleet_view_states(&mut self) {
        let mut exec_states: std::collections::HashMap<String, ThreadExecState> =
            self.fleet.iter().map(|entry| (entry.0.clone(), entry.1.exec_state)).collect();

        let focused = cp_mod_threads::types::FocusState::get(&self.state).focused_thread_id.clone();
        if let Some(id) = focused {
            let errs = cp_mod_spine::types::SpineState::get(&self.state).config.consecutive_continuation_errors;
            let status = ThreadsState::get(&self.state).threads.iter().find(|t| t.id == id).map(|t| t.status);
            let resident = Self::exec_state_from_residency(self.state.stream.phase.is_streaming(), errs, status);
            let _inserted = exec_states.insert(id, resident);
        }

        cp_mod_threads::view_state::FleetExecMirror::get_mut(&mut self.state).replace_all(exec_states);
    }

    /// The per-thread advancement core: drain this thread's stream, retry a
    /// failed request, flush the typewriter, **resolve its pending blocking
    /// waits**, run its tool pipeline, finalize a completed stream, and evaluate
    /// its spine. Called once per background thread while it is swapped in.
    /// Fleet-global work (bridge, file-watch events, cache channel drain,
    /// reverie) is NOT here — it runs once per tick, not once per thread.
    ///
    /// The three wait-resolution checks
    /// ([`check_waiting_for_panels`](super::super::tools::checks::check_waiting_for_panels),
    /// [`check_deferred_sleep`](super::super::tools::checks::check_deferred_sleep),
    /// [`check_watchers`](super::super::tools::cleanup::check_watchers)) MUST run
    /// here, mirroring the resident pipeline in
    /// [`run_background_phase`](super::App::run_background_phase). Without them a
    /// background thread that issues a blocking tool (e.g. `console_easy_bash`
    /// "sleep 3") parks on `pending_console_wait_tool_results` + a blocking
    /// `ConsoleWatcher` in its OWN (now swapped-in) registry, but that registry
    /// is only ever polled for the resident — so the sentinel is never replaced,
    /// `finalize_stream` keeps bailing, and the thread stalls permanently while
    /// the focused thread (which does get these checks) advances. They operate
    /// only on per-thread swapped-in state (the thread's `WatcherRegistry` plus
    /// its `StreamRuntime` accumulators), so running them here targets this
    /// thread and this thread only.
    fn step_one_thread(&mut self) {
        super::super::streaming::process_stream_events(self);
        super::super::streaming::handle_retry(self);
        super::super::streaming::process_typewriter(self);
        // Resolve this thread's pending blocking waits (panel refresh, deferred
        // tool-sleep, blocking/async watchers) — the fix for the N>1 background
        // console-wait stall. See the doc comment above.
        super::super::tools::checks::check_waiting_for_panels(self);
        super::super::tools::checks::check_deferred_sleep(self);
        super::super::tools::cleanup::check_watchers(self);
        super::super::tools::pipeline::handle_tool_execution(self);
        super::super::streaming::finalize_stream(self);
        self.check_spine();
    }
}
