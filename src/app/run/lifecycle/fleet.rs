//! Background-thread advancement — the multi-thread loop step (Phase C).
//!
//! The focused agent is stepped by `run_background_phase` in the parent
//! [`lifecycle`](super) module; this module advances every *other* active
//! agent by making it the executing agent for one step
//! (`agent_store.set_executing`), then restoring the previous one. No runtime
//! moves: each [`AgentRuntime`] stays in its own `AgentStore` slot. At N=1
//! the registry is empty and the pass is a no-op.

use cp_fleet::{AgentExecState, K, exec_state_from_facts, promote};
use cp_mod_agents::types::{AgentStatus, AgentsState};

use cp_base::state::runtime::bundle::AgentRuntime;

use crate::app::App;

#[expect(clippy::multiple_inherent_impl, reason = "App methods split across run/ submodules for readability")]
impl App {
    /// Make the **focused** agent the executing one when focus changes.
    ///
    /// Every agent's runtime lives permanently in
    /// [`AgentStore`](cp_base::state::runtime::agents::AgentStore); switching
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
    ///   adopts the boot-assembled unbound runtime; otherwise (cold agent) it
    ///   gets a freshly initialized one.
    /// - `want` is `None` (focus cleared): the unbound runtime becomes current,
    ///   re-initialized so per-agent module states exist for the next render.
    pub(super) fn follow_focus(&mut self) {
        let want = cp_mod_agents::types::FocusState::get(&self.state).focused_agent_id.clone();
        let have = self.state.executing_agent_id().map(str::to_owned);
        if want == have {
            return;
        }

        // A deleted outgoing agent (slot gone) needs no fleet entry.
        if let Some(have_id) = have.as_deref().filter(|id| self.state.agent_store.contains(id)) {
            self.park_outgoing(have_id.to_owned());
        }
        self.place_incoming(want.as_deref(), have.is_none());
        self.state.agent_store.set_executing(want);
    }

    /// Register the outgoing agent for background stepping. Its runtime,
    /// stream runtime included, stays in its own slot.
    /// A mid-exec agent is marked `Streaming`, not `Idle`: its `AgentStatus`
    /// is `TheirTurn` between turns, so `Idle` would drop it from scheduling.
    /// `post_step_exec_state` re-derives the real state on its first step.
    /// Runs while the outgoing agent is still the executing one.
    fn park_outgoing(&mut self, have_id: String) {
        let mid_exec = self.state.agent().stream.phase.is_streaming()
            || self.stream_rt().is_some_and(|sr| {
                sr.pending_console_wait_tool_results.is_some()
                    || !sr.pending_tools.is_empty()
                    || sr.pending_done.is_some()
            });
        let state = if mid_exec { AgentExecState::Streaming } else { AgentExecState::Idle };
        self.fleet.insert(have_id, state);
    }

    /// Give the incoming agent a stored runtime if it has none (the boot
    /// unbound runtime on `first_placement`, else a fresh one), take it out of
    /// the background registry. `None`
    /// (focus cleared) re-initializes the unbound runtime instead.
    fn place_incoming(&mut self, want: Option<&str>, first_placement: bool) {
        let Some(want_id) = want else {
            let fresh = self.fresh_runtime();
            drop(self.state.agent_store.take_unbound(fresh));
            return;
        };
        if !self.state.agent_store.contains(want_id) {
            let runtime = if first_placement {
                self.state.agent_store.take_unbound(AgentRuntime::new())
            } else {
                self.fresh_runtime()
            };
            self.state.agent_store.insert(want_id.to_owned(), runtime);
        }
        let _removed = self.fleet.remove(want_id);
    }

    /// A freshly initialized runtime (per-agent module states + fixed base
    /// panels), minting panel UIDs from the shared counter.
    pub(super) fn fresh_runtime(&mut self) -> AgentRuntime {
        let active = self.state.active_modules.clone();
        crate::state::persistence::fresh_agent_runtime(&mut self.state.global_next_uid, &active)
    }

    /// Reconcile the fleet registry against [`AgentsState`] — the roster-mirror
    /// that keeps [`fleet`](crate::app::App::fleet) in step with the agent list
    /// each tick, before [`advance_background_agents`](Self::advance_background_agents)
    /// steps the schedulable ones.
    ///
    /// Reconcile rules (the **focused** agent has no entry: it is stepped by
    /// the main pipeline, not the background loop):
    /// - a non-focused, non-archived agent missing from the registry gets a
    ///   fresh `Idle` [`Entry`], and an initialized runtime if it has none;
    /// - an entry whose agent vanished, was archived, or became focused is
    ///   removed (a vanished or archived agent also loses its runtime);
    /// - each surviving **non-active** entry's `exec_state` is derived from its
    ///   agent status (`MyTurn` → `Runnable`, else `Idle`); active entries are
    ///   left to the step loop (see [`FleetRegistry::derive_from_turn`](cp_fleet::FleetRegistry::derive_from_turn)).
    ///
    /// Promotion + stepping live in `advance_background_agents`; reconcile only
    /// mirrors membership and the Idle↔Runnable derivation.
    pub(super) fn reconcile_fleet_registry(&mut self) {
        let focused = cp_mod_agents::types::FocusState::get(&self.state).focused_agent_id.clone();

        // Snapshot the current roster: (id, status, eligible) for every agent
        // that should own a registry entry (non-focused, non-archived). One
        // pass over the groups builds the MY_TURN set (O(groups + agents)).
        let runnable = cp_groups::social::index::runnable_set(&cp_groups::types::GroupsState::get(&self.state).groups);
        let roster: Vec<(String, AgentStatus)> = AgentsState::get(&self.state)
            .agents
            .iter()
            .filter(|t| !t.archived && focused.as_deref() != Some(t.id.as_str()))
            .map(|t| {
                let status =
                    if runnable.contains(t.id.as_str()) { AgentStatus::MyTurn } else { AgentStatus::TheirTurn };
                (t.id.clone(), status)
            })
            .collect();
        drop(runnable);

        // Drop entries whose agent is gone / archived / now focused. A gone or
        // archived agent also loses its stored runtime; the focused one keeps it.
        let live: std::collections::HashSet<&str> = roster.iter().map(|entry| entry.0.as_str()).collect();
        let stale: Vec<String> =
            self.fleet.iter().map(|entry| entry.0.clone()).filter(|id| !live.contains(id.as_str())).collect();
        for id in stale {
            let _removed = self.fleet.remove(&id);
            if focused.as_deref() != Some(id.as_str()) {
                drop(self.state.agent_store.remove(&id));
            }
        }

        // Add missing entries, then derive exec_state for each roster agent.
        for entry in &roster {
            let (id, status) = (&entry.0, entry.1);
            if !self.state.agent_store.contains(id) {
                // A cold agent (in the roster, no persisted file) still needs a
                // runtime whose per-agent module states are INITIALIZED: a bare
                // `AgentRuntime::new()` has an empty module map, so its first
                // step's `ext::<SpineState>()` would panic.
                let runtime = self.fresh_runtime();
                self.state.agent_store.insert(id.clone(), runtime);
            }
            self.fleet.ensure(id);
            self.fleet.derive_from_turn(id, status == AgentStatus::MyTurn);
        }

        // Promotion is applied by `advance_background_agents` (it needs to
        // STEP the promoted agents, so the decision lives there). Reconcile is
        // purely the roster-mirror + Idle<->Runnable derivation above.
    }

    /// Boot-load every background agent's persisted runtime into the
    /// `AgentStore` and register it in the fleet — the boot half of F1 (design doc I-10 "boot loads N →
    /// registry"). Called once at the start of [`run`](super::App::run), before
    /// the loop's first [`reconcile_fleet_registry`](Self::reconcile_fleet_registry).
    ///
    /// For each non-focused, non-archived agent it calls
    /// [`boot_load_agent_runtime`](crate::state::persistence::boot_load_agent_runtime),
    /// which returns the agent's persisted [`AgentRuntime`] or `None` when the
    /// agent has no `states/<tid>.json` yet (a cold agent). A loaded runtime is
    /// stored and registered with its `exec_state` derived from the agent's
    /// status; a cold agent is left for `reconcile_fleet_registry`, which gives
    /// it a fresh initialized runtime on the first tick.
    ///
    /// At N=1 (and for any agent whose background agents have not yet been
    /// persisted per-agent) every lookup is `None`, so the fleet stays empty and
    /// boot is byte-identical: reconcile then behaves exactly as before.
    pub(super) fn load_background_agents(&mut self) {
        let focused = cp_mod_agents::types::FocusState::get(&self.state).focused_agent_id.clone();
        let roster: Vec<(String, AgentStatus)> = AgentsState::get(&self.state)
            .agents
            .iter()
            .filter(|t| !t.archived && focused.as_deref() != Some(t.id.as_str()))
            .map(|t| (t.id.clone(), cp_mod_agents::types::turn::status(&self.state, &t.id)))
            .collect();

        let active = self.state.active_modules.clone();
        for (id, status) in roster {
            let Some(runtime) =
                crate::state::persistence::boot_load_agent_runtime(&id, &mut self.state.global_next_uid, &active)
            else {
                continue; // cold agent: no persisted file; reconcile gives it a fresh runtime
            };
            self.state.agent_store.insert(id.clone(), runtime);
            self.fleet.ensure(&id);
            self.fleet.derive_from_turn(&id, status == AgentStatus::MyTurn);
        }
    }

    /// Advance every schedulable *background* (non-focused, capped) agent one
    /// step: the currently-active ones (continuing their stream) plus the ones
    /// [`promote`] selects to fill free concurrency slots (their kickoff step).
    ///
    /// For each, it makes the agent the executing one (and the stepping agent,
    /// so stream spawn/drain key by it), runs the advancement core
    /// ([`step_one_agent`](Self::step_one_agent)) — whose `check_spine` may
    /// start the agent's stream — re-derives the entry's [`AgentExecState`]
    /// from the post-step stream phase, then restores the previous executing
    /// agent. Switching is an id change; nothing is copied. Empty at N=1 → no-op.
    pub(super) fn advance_background_agents(&mut self) {
        let focused = cp_mod_agents::types::FocusState::get(&self.state).focused_agent_id.clone();
        // The focused agent is outside the registry; its live stream still takes a slot.
        let focused_active = usize::from(self.state.agent().stream.phase.is_streaming());
        let mut ids = self.fleet.ids_in(AgentExecState::Streaming);
        ids.extend(promote(&self.fleet, K, focused_active));
        ids.retain(|id| focused.as_deref() != Some(id.as_str()));

        let prev = self.state.executing_agent_id().map(str::to_owned);
        for id in ids {
            if !self.fleet.contains(&id) || !self.state.agent_store.contains(&id) {
                continue;
            }
            // Execute this thread: its stream spawn/drain, tee tag and every
            // per-agent write now target ITS runtime. No data moves.
            self.stepping_agent = Some(id.clone());
            self.state.agent_store.set_executing(Some(id.clone()));
            self.step_one_agent();
            let exec_state = self.post_step_exec_state(&id); // from this agent's new stream phase
            self.stepping_agent = None;
            self.state.agent_store.set_executing(prev.clone());
            let _known = self.fleet.set_state(&id, exec_state);
        }
    }

    /// Re-derive a just-stepped background agent's [`AgentExecState`] from the
    /// state it left behind (the agent is still the executing one).
    ///
    /// Delegates to [`exec_state_from_facts`]
    /// so the step loop and the display mirror cannot drift apart.
    fn post_step_exec_state(&self, id: &str) -> AgentExecState {
        let errs = cp_mod_spine::types::SpineState::get(&self.state).config.consecutive_continuation_errors;
        let status = AgentsState::get(&self.state)
            .agents
            .iter()
            .find(|t| t.id == id)
            .map(|t| cp_mod_agents::types::turn::status(&self.state, &t.id));
        exec_state_from_facts(self.state.agent().stream.phase.is_streaming(), errs, status == Some(AgentStatus::MyTurn))
    }

    /// Republish the display-only exec-state mirror
    /// ([`FleetExecMirror`](cp_mod_agents::view_state::FleetExecMirror)) from the
    /// registry plus the focused agent's own facts.
    ///
    /// Called once per tick, **after** the step loop, so the map reflects
    /// post-step derivations rather than the pre-step reconcile. Rebuilding
    /// wholesale (not merging) is what lets a deleted or archived agent vanish
    /// from the UI instead of lingering.
    ///
    /// The **focused** agent needs its own source: the registry excludes it, so
    /// a registry-only mirror would leave the row the human is looking at blank.
    /// Its stream phase and error streak are read from the executing agent,
    /// which is the focused one while the loop is at rest.
    pub(super) fn publish_fleet_view_states(&mut self) {
        let mut exec_states: std::collections::HashMap<String, AgentExecState> =
            self.fleet.iter().map(|entry| (entry.0.clone(), entry.1.exec_state())).collect();

        let focused = cp_mod_agents::types::FocusState::get(&self.state).focused_agent_id.clone();
        if let Some(id) = focused {
            let errs = cp_mod_spine::types::SpineState::get(&self.state).config.consecutive_continuation_errors;
            let status = AgentsState::get(&self.state)
                .agents
                .iter()
                .find(|t| t.id == id)
                .map(|t| cp_mod_agents::types::turn::status(&self.state, &t.id));
            let focused_state = exec_state_from_facts(
                self.state.agent().stream.phase.is_streaming(),
                errs,
                status == Some(AgentStatus::MyTurn),
            );
            let _inserted = exec_states.insert(id, focused_state);
        }

        cp_mod_agents::view_state::FleetExecMirror::get_mut(&mut self.state).replace_all(exec_states);
    }

    /// The per-agent advancement core: drain this agent's stream, retry a
    /// failed request, flush the typewriter, **resolve its pending blocking
    /// waits**, run its tool pipeline, finalize a completed stream, and evaluate
    /// its spine. Called once per background agent while it is executing.
    /// Fleet-global work (bridge, file-watch events, cache channel drain,
    /// reverie) is NOT here — it runs once per tick, not once per agent.
    ///
    /// The three wait-resolution checks
    /// ([`check_waiting_for_panels`](super::super::tools::checks::check_waiting_for_panels),
    /// [`check_deferred_sleep`](super::super::tools::checks::check_deferred_sleep),
    /// [`check_watchers`](super::super::tools::cleanup::check_watchers)) MUST run
    /// here, mirroring the focused pipeline in
    /// [`run_background_phase`](super::App::run_background_phase). Without them a
    /// background thread that issues a blocking tool (e.g. `console_easy_bash`
    /// "sleep 3") parks on `pending_console_wait_tool_results` + a blocking
    /// `ConsoleWatcher` in its OWN registry, but without these checks that
    /// registry is only polled for the focused agent — the sentinel is never replaced,
    /// `finalize_stream` keeps bailing, and the agent stalls permanently while
    /// the focused agent advances. They operate only on the executing agent's
    /// data (its `WatcherRegistry` plus its `StreamRuntime` accumulators), so
    /// running them here targets this agent and this agent only.
    fn step_one_agent(&mut self) {
        {
            let _g = crate::profile!("bg_stream_events");
            super::super::streaming::process_stream_events(self);
        }
        {
            let _g = crate::profile!("bg_retry");
            super::super::streaming::handle_retry(self);
        }
        {
            let _g = crate::profile!("bg_typewriter");
            super::super::streaming::process_typewriter(self);
        }
        // Resolve this thread's pending blocking waits (panel refresh, deferred
        // tool-sleep, blocking/async watchers) — the fix for the N>1 background
        // console-wait stall. See the doc comment above.
        {
            let _g = crate::profile!("bg_waiting_for_panels");
            super::super::tools::checks::check_waiting_for_panels(self);
        }
        {
            let _g = crate::profile!("bg_deferred_sleep");
            super::super::tools::checks::check_deferred_sleep(self);
        }
        {
            let _g = crate::profile!("bg_watchers");
            super::super::tools::cleanup::check_watchers(self);
        }
        super::super::tools::pipeline::handle_tool_execution(self);
        {
            let _g = crate::profile!("bg_finalize_stream");
            super::super::streaming::finalize_stream(self);
        }
        let _g = crate::profile!("bg_check_spine");
        self.check_spine();
    }
}
