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
    /// Relocate the resident bundle so the flat per-thread fields in
    /// [`state`](crate::app::App::state) always hold the **focused** thread —
    /// the thread-centric invariant "resident = focused" (design doc §4), made
    /// true on every focus change rather than only at boot.
    ///
    /// Focus changes (today: the agent's `Read`; later: a human drill-in) only
    /// set [`FocusState::focused_thread_id`](cp_mod_threads::types::FocusState);
    /// they do **not** move the bundle. Without this step the next
    /// [`reconcile_fleet_registry`](Self::reconcile_fleet_registry) would mislabel
    /// the old focused thread's live context (still flat in `state`) as the new
    /// focus and insert a *fresh empty* entry for the old one — losing its
    /// conversation. This primitive closes that gap: when the focus (`want`)
    /// differs from the resident (`have`), it parks `have`'s bundle into the
    /// registry and swaps `want`'s bundle out of the registry into `state`.
    ///
    /// Ordering: this MUST run before `reconcile_fleet_registry` (which assumes
    /// the invariant already holds) and before the focused pipeline steps, so the
    /// pipeline operates on the correct resident. It replaces the unconditional
    /// `resident_thread_id = focused` assignment at the top of
    /// [`run_background_phase`](super::App::run_background_phase).
    ///
    /// Cases:
    /// - `want == have` (including both `None`): no-op. This is the **only** path
    ///   at N=1 — a single-thread agent never switches focus, so the resident is
    ///   always already the focus and behaviour is byte-identical.
    /// - `want` is a background thread in the registry: park `have` (if any), then
    ///   swap `want`'s parked runtime into `state`.
    /// - `want` is cold/unknown (just created, never persisted): park `have`, leave
    ///   `state` with a fresh empty runtime — the correct blank view for a new
    ///   thread; `reconcile_fleet_registry` will not re-add it (it is the focus).
    ///
    /// Caveat (handled in later TC steps): if `have` was mid-stream when focus
    /// switched, its per-thread stream channel (keyed by its id) stops being
    /// drained until it is next stepped as a background thread. Focus is switched
    /// between turns in practice, so the common path parks an idle thread.
    pub(super) fn relocate_resident_on_focus_change(&mut self) {
        let want = cp_mod_threads::types::FocusState::get(&self.state).focused_thread_id.clone();
        let have = self.state.resident_thread_id.clone();
        let parking_resident = have.is_some();
        if want == have {
            // Keep the label in sync for the None/None and equal cases, then done.
            self.state.resident_thread_id = want;
            return;
        }

        // Park the current resident's live bundle back into the registry, so its
        // context is preserved rather than being overwritten by the swap-in below
        // (and not mislabelled as the new focus by reconcile).
        if let Some(have_id) = have {
            let _g = crate::profile!("rr_park");
            let mut parked = cp_base::state::runtime::bundle::ThreadRuntime::new();
            {
                let _bundle = crate::profile!("rr_park_bundle");
                parked.swap_with(&mut self.state); // `parked` now holds `have`'s context; state emptied
            }
            // Park the resident's per-stream runtime alongside its bundle, so its
            // in-flight typewriter/pending-tools travel with it rather than
            // leaking into the newly-focused thread.
            let mut sr = super::stream_runtime::StreamRuntime::new();
            sr.swap_with_app(self); // `sr` now holds `have`'s per-stream runtime; App reset to empty
            // Is the outgoing resident mid-execution? A live stream, a pending
            // blocking console-wait, un-executed tool calls, or a deferred
            // `StreamDone` all mean "this thread is doing work right now" —
            // independent of its MyTurn/TheirTurn conversation status.
            let mid_exec = parked.stream.phase.is_streaming()
                || sr.pending_console_wait_tool_results.is_some()
                || !sr.pending_tools.is_empty()
                || sr.pending_done.is_some();
            let _prev = self.parked_stream_runtimes.insert(have_id.clone(), sr);
            // Preserve an existing entry's role if one somehow exists; otherwise a
            // plain Thread entry.
            let role = self.fleet.get(&have_id).map_or(Role::Thread, |e| e.role);
            let mut entry = Entry::new(role, parked);
            // Park a mid-execution thread as `Streaming` (active) so the step loop
            // keeps advancing it and `reconcile`'s status-only `derive_exec_state`
            // (which early-returns for active entries) does NOT clobber it to Idle.
            // Without this, a thread counting via one blocking tool per step —
            // whose `ThreadStatus` is `TheirTurn` between turns — would be parked
            // Idle on focus-switch, dropped from scheduling, and stall the instant
            // it stops being focused (the focused resident is stepped
            // unconditionally; a background thread is gated by exec_state). The
            // first background step re-derives the true state from its stream phase
            // via `post_step_exec_state`.
            if mid_exec {
                entry.exec_state = ThreadExecState::Streaming;
            }
            self.fleet.insert(have_id, entry);
        }

        // Swap the newly-focused thread's parked bundle into `state`. A cold or
        // unknown thread has no entry → state keeps the fresh empty runtime, which
        // is the correct blank view for a brand-new thread.
        let swapped_in = {
            let _g = crate::profile!("rr_swap_in");
            want.as_deref().is_some_and(|want_id| self.swap_in_parked(want_id))
        };

        // The park above emptied `state` to a bare `ThreadRuntime::new()` (no
        // per-thread module states). If nothing was swapped back in — a cold
        // thread, or focus cleared because the focused thread was just archived —
        // the next render's `ext::<SpineState>()` would panic. Install an
        // initialized blank runtime instead.
        if parking_resident && !swapped_in {
            let _g = crate::profile!("rr_install_blank");
            self.install_blank_resident();
        }

        self.state.resident_thread_id = want;
    }

    /// Swap `want_id`'s parked bundle and per-stream runtime into `state`/App.
    /// Returns whether a parked bundle existed (a cold thread has none).
    fn swap_in_parked(&mut self, want_id: &str) -> bool {
        let swapped_in = self.fleet.remove(want_id).is_some_and(|mut entry| {
            let _g = crate::profile!("rr_swap_bundle");
            entry.runtime.swap_with(&mut self.state); // state now holds `want`'s context; leftover dropped
            true
        });
        // A cold thread has no parked per-stream runtime: App stays at the empty default.
        if let Some(mut sr) = self.parked_stream_runtimes.remove(want_id) {
            let _g = crate::profile!("rr_swap_stream");
            sr.swap_with_app(self); // App now holds `want`'s per-stream runtime; leftover dropped
        }
        swapped_in
    }

    /// Swap a freshly-initialized blank runtime (per-thread modules + fixed base
    /// panels, via [`fresh_thread_runtime`](crate::state::persistence::fresh_thread_runtime))
    /// into `state`, replacing the uninitialized leftover of a park.
    fn install_blank_resident(&mut self) {
        let active = self.state.active_modules.clone();
        let mut fresh = crate::state::persistence::fresh_thread_runtime(&mut self.state.global_next_uid, &active);
        fresh.swap_with(&mut self.state); // `fresh` now holds the empty leftover — dropped
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
                // A cold thread (in the roster, no persisted file) must still get
                // a runtime whose per-thread module states are INITIALIZED — a
                // bare `ThreadRuntime::new()` has an empty module map, so once the
                // step loop promotes and swaps it in, `check_spine`'s first
                // `ext::<SpineState>()` would panic. `fresh_thread_runtime` inits
                // the per-thread modules + fixed base panels exactly like a disk
                // load would, minting panel UIDs from the shared counter.
                let active = self.state.active_modules.clone();
                let runtime = crate::state::persistence::fresh_thread_runtime(&mut self.state.global_next_uid, &active);
                self.fleet.insert(id.clone(), Entry::new(Role::Thread, runtime));
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
            // Swap this thread's per-stream runtime (typewriter, pending tools/
            // done, console-wait + blocking accumulators, deferred-sleep flags)
            // into `App` so the shared advancement core drains ITS buffers, not
            // the focused thread's — without this, the focused thread's residual
            // typewriter chars / pending tools bleed into this thread (N>1 bug).
            let mut sr = self.parked_stream_runtimes.remove(&id).unwrap_or_default();
            sr.swap_with_app(self);
            self.step_one_thread();
            entry.exec_state = self.post_step_exec_state(&id); // from this thread's new stream phase
            sr.swap_with_app(self); // restore focused thread's per-stream runtime
            let _prev = self.parked_stream_runtimes.insert(id.clone(), sr);
            entry.runtime.swap_with(&mut self.state); // restore focused; thread `id` parks back
            self.stepping_thread = None;
            self.state.resident_thread_id.clone_from(&focused);
            self.fleet.insert(id, entry);
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
