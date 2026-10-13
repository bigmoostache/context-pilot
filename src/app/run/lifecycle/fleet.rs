//! Background-thread advancement — the multi-thread loop step (Phase C).
//!
//! The focused agent is stepped by `run_background_phase` in the parent
//! [`lifecycle`](super) module; this module advances every *other* active
//! agent by making it the executing agent for one step
//! (`agent_store.set_executing`), then restoring the previous one. No runtime
//! moves: each [`AgentRuntime`] stays in its own `AgentStore` slot. At N=1
//! the registry is empty and the pass is a no-op.

use cp_fleet::{AgentExecState, Entry, Role, promote};
use cp_mod_agents::types::{AgentStatus, AgentsState};

use cp_base::state::runtime::bundle::AgentRuntime;

use crate::app::App;

/// Consecutive terminal stream failures (each after its own API retries are
/// exhausted) before a background agent is parked as
/// [`Errored`](AgentExecState::Errored).
///
/// A one-off blip stays `Runnable` and retries after the spine's exponential
/// backoff; only an agent that keeps failing is declared *stuck* and removed
/// from scheduling until a human re-engages it (design doc §8 — human
/// intervention is the recovery path for a stuck agent). The counter
/// (`SpineState.config.consecutive_continuation_errors`) is reset to 0 by a
/// successful stream completion or a fresh user message.
const STUCK_ERROR_THRESHOLD: usize = 3;

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
        let role = self.fleet.get(&have_id).map_or(Role::Agent, |e| e.role);
        let mut entry = Entry::new(role);
        if mid_exec {
            entry.exec_state = AgentExecState::Streaming;
        }
        self.fleet.insert(have_id, entry);
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
    ///   left to the step loop (see [`derive_exec_state`](Self::derive_exec_state)).
    ///
    /// Promotion + stepping live in `advance_background_agents`; reconcile only
    /// mirrors membership and the Idle↔Runnable derivation.
    pub(super) fn reconcile_fleet_registry(&mut self, now_ms: u64) {
        let focused = cp_mod_agents::types::FocusState::get(&self.state).focused_agent_id.clone();

        // Snapshot the current roster: (id, status, eligible) for every agent
        // that should own a registry entry (non-focused, non-archived).
        let roster: Vec<(String, AgentStatus)> = AgentsState::get(&self.state)
            .agents
            .iter()
            .filter(|t| !t.archived && focused.as_deref() != Some(t.id.as_str()))
            .map(|t| (t.id.clone(), t.status))
            .collect();

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
            if !self.fleet.contains(id) {
                self.fleet.insert(id.clone(), Entry::new(Role::Agent));
            }
            if let Some(reg_entry) = self.fleet.get_mut(id) {
                Self::derive_exec_state(reg_entry, status, now_ms);
            }
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
        let now = cp_base::panels::now_ms();
        let roster: Vec<(String, AgentStatus)> = AgentsState::get(&self.state)
            .agents
            .iter()
            .filter(|t| !t.archived && focused.as_deref() != Some(t.id.as_str()))
            .map(|t| (t.id.clone(), t.status))
            .collect();

        let active = self.state.active_modules.clone();
        for (id, status) in roster {
            let Some(runtime) =
                crate::state::persistence::boot_load_agent_runtime(&id, &mut self.state.global_next_uid, &active)
            else {
                continue; // cold agent: no persisted file; reconcile gives it a fresh runtime
            };
            self.state.agent_store.insert(id.clone(), runtime);
            let mut entry = Entry::new(Role::Agent);
            Self::derive_exec_state(&mut entry, status, now);
            self.fleet.insert(id, entry);
        }
    }

    /// Derive one background entry's [`AgentExecState`] from its agent status.
    ///
    /// An **active** entry ([`Streaming`](AgentExecState::Streaming) /
    /// [`AwaitingLlm`](AgentExecState::AwaitingLlm)) is owned by the advancement
    /// step loop ([`advance_background_agents`](Self::advance_background_agents)
    /// re-derives it from the post-step stream phase), so reconcile leaves it
    /// untouched — otherwise a mid-stream agent, whose `AgentStatus` is still
    /// `MyTurn`, would be clobbered back to `Runnable` every tick.
    ///
    /// For a non-active entry: `MyTurn` means user input awaits the agent →
    /// `Runnable` (stamping `waiting_since_ms` once, on the Idle→Runnable edge,
    /// for oldest-waiting-first promotion); `TheirTurn` → `Idle` (waiting on the
    /// human), clearing any stale wait stamp.
    fn derive_exec_state(entry: &mut Entry, status: AgentStatus, now_ms: u64) {
        if entry.exec_state.is_active() || entry.exec_state == AgentExecState::Errored {
            // Active: owned by the advancement step loop (re-derived post-step).
            // Errored: parked as stuck — never auto-revived here; only a fresh
            // user message clears it (see `clear_errored_entry`), matching the
            // human-intervention recovery model. Never clobber either back to
            // Runnable just because the agent's status is still MyTurn.
            return;
        }
        match status {
            AgentStatus::MyTurn => {
                if entry.exec_state != AgentExecState::Runnable {
                    entry.exec_state = AgentExecState::Runnable;
                    entry.waiting_since_ms = Some(now_ms);
                }
            }
            AgentStatus::TheirTurn => {
                entry.exec_state = AgentExecState::Idle;
                entry.waiting_since_ms = None;
            }
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
            if let Some(entry) = self.fleet.get_mut(&id) {
                entry.exec_state = exec_state;
            }
        }
    }

    /// Re-derive a just-stepped background agent's [`AgentExecState`] from the
    /// state it left behind (the agent is still the executing one).
    ///
    /// Delegates to [`exec_state_from_facts`](Self::exec_state_from_facts)
    /// so the step loop and the display mirror cannot drift apart.
    fn post_step_exec_state(&self, id: &str) -> AgentExecState {
        let errs = cp_mod_spine::types::SpineState::get(&self.state).config.consecutive_continuation_errors;
        let status = AgentsState::get(&self.state).agents.iter().find(|t| t.id == id).map(|t| t.status);
        Self::exec_state_from_facts(self.state.agent().stream.phase.is_streaming(), errs, status)
    }

    /// The single definition of "what exec state do these agent facts imply".
    ///
    /// Shared by two callers that must agree: the step loop's post-step
    /// derivation ([`post_step_exec_state`](Self::post_step_exec_state)) and the
    /// display mirror ([`publish_fleet_view_states`](Self::publish_fleet_view_states)).
    /// A second, drifting definition would show the human a state the scheduler
    /// never actually held.
    ///
    /// Order matters: a live stream wins (the agent holds its `K` slot), then a
    /// fatal error streak parks it as `Errored`, and only then does the turn
    /// status decide `Runnable` vs `Idle`.
    const fn exec_state_from_facts(
        is_streaming: bool,
        consecutive_errors: usize,
        status: Option<AgentStatus>,
    ) -> AgentExecState {
        if is_streaming {
            return AgentExecState::Streaming;
        }
        // Repeated terminal failures → park as Errored (stuck, needs a human).
        // Excluded from both `promote` and the step loop, so a persistently
        // failing agent stops churning a promotion slot and can never stall the
        // fleet; it waits for a fresh user message to re-engage it.
        if consecutive_errors >= STUCK_ERROR_THRESHOLD {
            return AgentExecState::Errored;
        }
        match status {
            Some(AgentStatus::MyTurn) => AgentExecState::Runnable,
            _ => AgentExecState::Idle,
        }
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
            self.fleet.iter().map(|entry| (entry.0.clone(), entry.1.exec_state)).collect();

        let focused = cp_mod_agents::types::FocusState::get(&self.state).focused_agent_id.clone();
        if let Some(id) = focused {
            let errs = cp_mod_spine::types::SpineState::get(&self.state).config.consecutive_continuation_errors;
            let status = AgentsState::get(&self.state).agents.iter().find(|t| t.id == id).map(|t| t.status);
            let focused_state =
                Self::exec_state_from_facts(self.state.agent().stream.phase.is_streaming(), errs, status);
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
