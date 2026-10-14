//! Fleet lifecycle I/O — the boot/persist/teardown half of the multi-thread
//! model (Phase F), split from the sibling [`fleet`](super::fleet) scheduling
//! module to keep each file under the 500-line cap.
//!
//! `fleet` owns the per-tick *scheduling* (reconcile, promote, advance, the
//! [`deliver_to_agent`](crate::app::App::deliver_to_agent) delivery seam);
//! this file owns the *lifecycle* operations that touch disk and external
//! resources: the fleet-wide console orphan-prune (F6), the N-thread save
//! (F1), per-agent hard-delete teardown (F5), and the human-intervention
//! re-engage of an `Errored` agent (F4).

use cp_fleet::AgentExecState;
use cp_mod_agents::types::{AgentStatus, AgentsState};

use crate::app::App;

/// Executing agent to restore after a drill-in paint (`None` = unbound).
pub(super) struct DrillRestore {
    /// Agent that was executing before the drilled agent was made current.
    prev: Option<String>,
}

#[expect(clippy::multiple_inherent_impl, reason = "App methods split across run/ submodules for readability")]
impl App {
    /// Kill console-server sessions that belong to **no** loaded agent, in a
    /// single fleet-wide pass over the union of every agent's session keys
    /// (design doc §F6 / S2 — the reattach-by-thread half of lifecycle).
    ///
    /// Why this is not in the console module's `load_module_data`: that hook runs
    /// once *per agent* at boot, but the console server's session map is shared
    /// fleet-wide. Killing orphans relative to one agent's keys would kill every
    /// *other* agent's live sessions (the exact class of bug as the F1c
    /// per-agent panel prune). So the per-agent kill was removed and this pass
    /// runs it once, after [`load_background_agents`](Self::load_background_agents),
    /// over the union of the focused agent's keys plus each background agent's keys.
    ///
    /// Session reconnection stays per-agent (each agent reattaches its own
    /// sessions when its module data loads); only the orphan *kill* is unioned.
    ///
    /// N=1-identical: the fleet is empty, so the union is just the focused
    /// agent's keys — exactly what the removed `load_module_data` kill used
    /// (and an empty set kills every orphan, matching the old empty-sessions
    /// branch).
    pub(super) fn prune_orphaned_console_sessions(&mut self) {
        use cp_mod_console::types::ConsoleState;

        // Executing (focused) agent's live session keys.
        let mut known: std::collections::HashSet<String> =
            ConsoleState::get(&self.state).sessions.keys().cloned().collect();

        // Union in each background thread's keys (pure read, no stream spawned).
        let bg_ids: Vec<String> = self.fleet.iter().map(|entry| entry.0.clone()).collect();
        for id in bg_ids {
            self.deliver_to_agent(Some(&id), |state| {
                known.extend(ConsoleState::get(state).sessions.keys().cloned());
            });
        }

        cp_mod_console::manager::kill_orphaned_processes(&known);
    }

    /// Persist **every** agent to disk — the focused agent first, then each
    /// background agent by making it the executing one and saving (design doc
    /// §F1, I-10). Called on reload and quit, replacing the single-thread
    /// `save_state`.
    ///
    /// Why this exists and not just `save_state`: `build_save_batch` deliberately
    /// no longer prunes orphaned `panels/<uid>.json` per call (that would delete
    /// *other* agents' panels — the dir is shared, keyed by the fleet-global UID
    /// counter). So each agent is saved with empty deletes here, their live
    /// panel UIDs are unioned via [`panel_uids_of`](crate::state::persistence::save::panel_uids_of),
    /// and the orphan-prune runs **exactly once** over that union at the end.
    ///
    /// `save_state` writes the executing agent to `states/<tid>.json`, so each
    /// save is preceded by `set_executing(tid)`; the previous executing agent is
    /// restored at the end. At N=1 this saves only the focused agent and prunes
    /// over its UIDs (boot loads from the persisted UID index, never a dir scan).
    pub(super) fn save_all_agents(&mut self) {
        use crate::state::persistence::save;

        let focused = cp_mod_agents::types::FocusState::get(&self.state).focused_agent_id.clone();
        let mut known: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Focused agent first.
        self.state.agent_store.set_executing(focused.clone());
        known.extend(save::panel_uids_of(&self.state));
        save::save_state(&self.state);

        // Each background agent: execute it, snapshot to its own file.
        let bg_ids: Vec<String> = self.fleet.iter().map(|entry| entry.0.clone()).collect();
        for id in bg_ids {
            if !self.state.agent_store.contains(&id) {
                continue;
            }
            self.state.agent_store.set_executing(Some(id));
            known.extend(save::panel_uids_of(&self.state));
            save::save_state(&self.state); // → states/<id>.json (executing != focused), no deletes
        }
        self.state.agent_store.set_executing(focused);

        // Union orphan-prune, exactly once over every agent's live UIDs.
        for del in save::collect_orphan_deletes(&save::panels_dir(), &known) {
            save::exec_delete_op(&del);
        }
    }

    /// Re-engage a background agent parked as [`Errored`](AgentExecState::Errored),
    /// flipping its registry entry back to `Runnable` so the loop schedules it again.
    ///
    /// This is the **human-intervention recovery path** (design doc §8): a stuck
    /// agent is only revived by a fresh user message, which `route_on_user_message`
    /// routes here after resetting the agent's spine error counters. A no-op when
    /// the thread has no entry (the focused thread, or an unknown thread) or when
    /// the entry is not `Errored` — so at N=1 (focused agent never in the registry)
    /// it never fires.
    pub(crate) fn clear_errored_entry(&mut self, agent_id: &str) {
        if let Some(entry) = self.fleet.get_mut(agent_id)
            && entry.exec_state == AgentExecState::Errored
        {
            entry.exec_state = AgentExecState::Runnable;
            entry.waiting_since_ms = Some(cp_base::panels::now_ms());
        }
    }

    /// Structural teardown of a **hard-deleted** agent (design doc §F5 / H15):
    /// kill its external console processes, then remove its runtime from the
    /// `AgentStore` (which
    /// drops the agent's per-agent resources — the [`WatcherRegistry`], queue,
    /// spine inbox — cancelling its in-process watchers by `Drop`).
    ///
    /// Called from `apply_command`'s `DeleteAgent` arm **after**
    /// [`apply_delete_agent`](super::super::agents::commands) has removed the
    /// agent from [`AgentsState`] and cleared focus/bridge memos. The agent id
    /// is the single teardown key.
    ///
    /// Console sessions are per-agent ([`ConsoleState`](cp_mod_console::types::ConsoleState)
    /// is `is_global() == false`), so
    /// [`deliver_to_agent`](Self::deliver_to_agent) makes the agent executing
    /// and [`shutdown_all`](cp_mod_console::types::ConsoleState::shutdown_all)
    /// kills exactly *its* sessions (by their own keys, via the existing
    /// per-session kill — no server protocol change). The in-process watcher
    /// drop is `Drop` on the removed
    /// [`AgentRuntime`](cp_base::state::runtime::bundle::AgentRuntime): a
    /// [`ChannelWatcher`](cp_base::state::watchers::ChannelWatcher) drops its
    /// receiver so its worker thread's later send fails harmlessly (cancellation);
    /// timer/console watchers are data-only.
    ///
    /// Deleting the focused agent runs `shutdown_all` directly and
    /// [`fleet.remove`](cp_fleet::FleetRegistry::remove) is a no-op (the focused
    /// agent is never in the registry). No console key or on-disk path changes.
    pub(crate) fn teardown_agent(&mut self, agent_id: &str) {
        self.deliver_to_agent(Some(agent_id), |state| {
            cp_mod_console::types::ConsoleState::shutdown_all(state);
        });
        let _removed = self.fleet.remove(agent_id);
        drop(self.state.agent_store.remove(agent_id));
        // Deleting the executing agent: fall back to an initialized unbound
        // runtime so the next render's per-agent `ext` lookups still resolve.
        if self.state.executing_agent_id() == Some(agent_id) {
            let fresh = self.fresh_runtime();
            drop(self.state.agent_store.take_unbound(fresh));
            self.state.agent_store.set_executing(None);
        }
    }

    /// Render-scoped drill-in: make the human-drilled agent
    /// ([`FocusState::drilled_agent_id`](cp_mod_agents::types::FocusState)) the
    /// executing one **for one paint only**. Returns the previous executing id,
    /// which the caller passes to
    /// [`restore_drilled_runtime_after_render`](Self::restore_drilled_runtime_after_render)
    /// right after `terminal.draw`.
    ///
    /// Focus-as-view and execution stay decoupled (Model 2): no data moves,
    /// `focused_agent_id` and scheduling are untouched, and a human glance never
    /// parks a mid-stream agent.
    ///
    /// `None` when there is no drill-in, the drilled agent is already executing,
    /// or it has no stored runtime (cold/unknown).
    pub(super) fn take_drilled_runtime_for_render(&mut self) -> Option<DrillRestore> {
        let drilled = cp_mod_agents::types::FocusState::get(&self.state).drilled_agent_id.clone()?;
        if self.state.executing_agent_id() == Some(drilled.as_str()) || !self.state.agent_store.contains(&drilled) {
            return None;
        }
        let prev = self.state.executing_agent_id().map(str::to_owned);
        self.state.agent_store.set_executing(Some(drilled));
        Some(DrillRestore { prev })
    }

    /// Undo [`take_drilled_runtime_for_render`](Self::take_drilled_runtime_for_render):
    /// restore the executing agent that was current before the paint.
    pub(super) fn restore_drilled_runtime_after_render(&mut self, restore: Option<DrillRestore>) {
        if let Some(r) = restore {
            self.state.agent_store.set_executing(r.prev);
        }
    }

    /// Nudge every **background** `MyTurn` agent that is idle and has no live
    /// notification, so [`check_spine`](crate::app::App::check_spine) continues
    /// it once it is promoted and stepped (the per-agent *dispatcher*, design
    /// doc §7.3).
    ///
    /// The **focused** agent is deliberately excluded — it is handled by the
    /// [`IdleMyTurnDetector`](cp_mod_agents::watcher::IdleMyTurnDetector) watcher
    /// (focused-only since this dispatcher subsumed its background fallback), so
    /// the two nudge paths never overlap on the same agent.
    ///
    /// For each candidate it runs against that agent (via
    /// [`deliver_to_agent`](Self::deliver_to_agent)) and, **only if** that
    /// agent is idle (not streaming) and its inbox holds no unprocessed
    /// notification, drops a single `Custom`/`agents` notification bound to it.
    /// Both guards make this flood-safe by construction: an agent mid-stream or
    /// already-nudged is skipped, and [`SpineState::create_notification`] itself
    /// dedups on `(kind, source)`, so re-running every tick never piles up.
    ///
    /// At N=1 the only `MyTurn` agent is the focused one (excluded), so the
    /// candidate set is empty and this is a no-op — byte-identical to
    /// single-thread.
    pub(super) fn dispatch_background_my_turn(&mut self) {
        let focused = cp_mod_agents::types::FocusState::get(&self.state).focused_agent_id.clone();
        let mirror = cp_mod_agents::view_state::FleetExecMirror::get(&self.state);
        // Snapshot (id, name) of eligible background agents before touching the
        // per-agent state (delivery below borrows `state`). Only truly idle
        // agents: one awaiting the LLM or running tools is mid-turn and not
        // streaming, and a nudge there lands after it has already replied.
        let candidates: Vec<(String, String)> = AgentsState::get(&self.state)
            .agents
            .iter()
            .filter(|t| {
                !t.archived
                    && !t.paused
                    && mirror.exec_state_of(&t.id) == AgentExecState::Idle
                    && cp_mod_agents::types::turn::status(&self.state, &t.id) == AgentStatus::MyTurn
                    && focused.as_deref() != Some(t.id.as_str())
            })
            .map(|t| (t.id.clone(), t.name.clone()))
            .collect();

        for (tid, name) in candidates {
            let tid_for_bind = tid.clone();
            self.deliver_to_agent(Some(&tid), move |state| {
                // Skip if this agent is already working or already nudged — the
                // two guards that keep the dispatcher from flooding an inbox.
                if state.agent().stream.phase.is_streaming() {
                    return;
                }
                if cp_mod_spine::types::SpineState::has_unprocessed_notifications(state) {
                    return;
                }
                // The turn may have been handed back since the snapshot.
                if cp_mod_agents::types::turn::status(state, &tid_for_bind) != AgentStatus::MyTurn {
                    return;
                }
                let content = cp_mod_agents::types::turn::nudge(state, &tid_for_bind, &name);
                let nid = cp_mod_spine::types::SpineState::create_notification(
                    state,
                    cp_mod_spine::types::NotificationType::Custom,
                    "agents".to_owned(),
                    content,
                );
                cp_mod_spine::types::SpineState::set_notification_agent(state, &nid, Some(tid_for_bind));
            });
        }
    }

    /// Advance every group's time-driven social substates (T860 D4).
    ///
    /// Due `ComingBack`s become `SimpleTurn`s and their reminder is delivered
    /// to the woken agent's spine inbox; each `Dispatch` then goes to its first
    /// idle candidate. Runs right before
    /// [`dispatch_background_my_turn`](Self::dispatch_background_my_turn), so
    /// a dispatch never picks an agent the same tick already relaunches.
    /// "Idle" = active, unpaused, and [`AgentExecState::Idle`] in the exec mirror.
    pub(super) fn tick_social(&mut self, now_ms: u64) {
        let mirror = cp_mod_agents::view_state::FleetExecMirror::get(&self.state);
        let idle: std::collections::HashSet<String> = AgentsState::get(&self.state)
            .agents
            .iter()
            .filter(|a| !a.archived && !a.paused && mirror.exec_state_of(&a.id) == AgentExecState::Idle)
            .map(|a| a.id.clone())
            .collect();
        let groups = &mut cp_groups::types::GroupsState::get_mut(&mut self.state).groups;
        let fired = cp_groups::social::tick(groups, now_ms, &|id: &str| idle.contains(id));
        for f in fired {
            let label = cp_groups::types::GroupsState::get(&self.state).label(&f.group_id);
            let content = format!("Coming back in {label}: {}", f.message);
            let bind = f.agent_id.clone();
            self.deliver_to_agent(Some(&f.agent_id), move |state| {
                let nid = cp_mod_spine::types::SpineState::create_notification(
                    state,
                    cp_mod_spine::types::NotificationType::Custom,
                    "social".to_owned(),
                    content,
                );
                cp_mod_spine::types::SpineState::set_notification_agent(state, &nid, Some(bind));
            });
        }
    }

    /// Run `f` against the state of the agent that owns `agent_id` — the
    /// **delivery seam** for thread-addressed spine routing (Phase D1).
    ///
    /// Delivery is distinct from advancement: it mutates an agent's own context
    /// (its spine inbox, its conversation) without stepping its pipeline. This is
    /// how a watcher fire, a coucou, or a bridge message for a *background*
    /// agent lands in *that* agent's inbox rather than the focused one's.
    ///
    /// Routing:
    /// - `None`, or the focused agent's id → run `f` on `state` as is;
    /// - any other stored agent → make it executing, run `f`, restore the
    ///   previous executing agent (an id change, nothing is copied).
    ///
    /// A `agent_id` with no stored runtime (unknown/archived) is logged and
    /// delivered to the executing agent as a last-resort safety net.
    pub(crate) fn deliver_to_agent<F>(&mut self, agent_id: Option<&str>, f: F)
    where
        F: FnOnce(&mut cp_base::state::runtime::State),
    {
        let focused = cp_mod_agents::types::FocusState::get(&self.state).focused_agent_id.clone();
        let Some(tid) = agent_id.filter(|t| focused.as_deref() != Some(*t)) else {
            // None, or targets the focused agent → deliver directly.
            f(&mut self.state);
            return;
        };
        if !self.state.agent_store.contains(tid) {
            log::warn!("deliver_to_thread: unknown agent {tid}; delivering to the executing agent");
            f(&mut self.state);
            return;
        }
        // Background owner: execute it for the duration of `f`, then restore.
        let prev = self.state.executing_agent_id().map(str::to_owned);
        self.state.agent_store.set_executing(Some(tid.to_owned()));
        f(&mut self.state);
        self.state.agent_store.set_executing(prev);
    }
}
