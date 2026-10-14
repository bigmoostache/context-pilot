//! Fleet-shared coucou delivery: poll the [`CoucouRegistry`] once per tick and
//! route each due reminder into its target agent's spine inbox, waking that
//! agent when it sits in the background. Plus the one-shot boot migration of
//! legacy per-agent `pending_coucous`.

use cp_mod_agents::types::{AgentsState, FocusState};
use cp_mod_spine::schedule::CoucouRegistry;
use cp_mod_spine::types::{NotificationType, SpineState};

use crate::app::App;

#[expect(clippy::multiple_inherent_impl, reason = "App methods split across run/ submodules for readability")]
impl App {
    /// Deliver every coucou due now. Runs on the main loop with the focused
    /// agent executing (never inside a background step), so
    /// [`deliver_to_agent`](Self::deliver_to_agent) resolves "focused" against
    /// the real executing. Unscoped coucous (`agent_id: None`) go to the focused
    /// agent.
    ///
    /// A background target is flipped to `MyTurn`: its inbox notification alone
    /// does not schedule it (only `MyTurn` agents are promoted), so without the
    /// flip a `THEIR_TURN` agent would hold the reminder unread until the human
    /// returned to it.
    pub(super) fn check_coucous(&mut self) {
        let fired = CoucouRegistry::get_mut(&mut self.state).take_due(cp_base::panels::now_ms());
        if fired.is_empty() {
            return;
        }
        let focused = FocusState::get(&self.state).focused_agent_id.clone();
        for mut coucou in fired {
            // Name the group: the record only knows its id.
            if let Some(gid) = coucou.group_id.as_deref() {
                let label = cp_groups::types::GroupsState::get(&self.state).label(gid);
                coucou.description = coucou.description.replacen(&format!(" in {gid}"), &format!(" in {label}"), 1);
            }
            let tid = coucou.agent_id.clone();
            let bind = tid.clone();
            // Source = coucou id: `create_notification` dedups on (kind, source),
            // so a shared source would swallow a second coucou firing together.
            self.deliver_to_agent(tid.as_deref(), |state| {
                let nid =
                    SpineState::create_notification(state, NotificationType::Custom, coucou.id, coucou.description);
                SpineState::set_notification_agent(state, &nid, bind);
            });
            if let Some(target) = tid.filter(|t| focused.as_deref() != Some(t.as_str()))
                && let Some(agent) = AgentsState::get(&self.state).agents.iter().find(|t| t.id == target)
                && !agent.archived
            {
                give_turn_in(&mut self.state, &target, coucou.group_id.as_deref());
            }
        }
        self.state.flags.ui.dirty = true;
        self.save_state_async();
    }

    /// Move coucous persisted by older builds in each agent's own spine slot
    /// into the fleet-shared registry. Each record keeps its explicit target, or
    /// is stamped with the agent that stored it, and gets a fresh id: the old
    /// per-agent counters all started at `coucou_0`, so the ids collide.
    /// Called once at boot, after background agents are loaded.
    pub(super) fn migrate_legacy_coucous(&mut self) {
        let focused = FocusState::get(&self.state).focused_agent_id.clone();
        let mut moved = drain_legacy(&mut self.state, focused.as_deref());

        let bg_ids: Vec<String> = self.fleet.iter().map(|entry| entry.0.clone()).collect();
        for id in bg_ids {
            self.deliver_to_agent(Some(&id), |state| moved.extend(drain_legacy(state, Some(&id))));
        }
        if moved.is_empty() {
            return;
        }
        let registry = CoucouRegistry::get_mut(&mut self.state);
        for mut coucou in moved {
            coucou.watcher_id = registry.alloc_id();
            registry.pending.push(coucou);
        }
        self.save_state_async();
    }
}

/// Give `agent_id` the turn in `group_id` when it is still a member there,
/// else in its human group.
fn give_turn_in(state: &mut cp_base::state::runtime::State, agent_id: &str, group_id: Option<&str>) {
    let groups = cp_groups::types::GroupsState::get_mut(state);
    if let Some(group) =
        group_id.and_then(|g| groups.find_mut(g)).filter(|g| g.member_ids.iter().any(|m| m == agent_id))
    {
        cp_groups::social::give_turn(group, agent_id);
        return;
    }
    cp_mod_agents::types::turn::give(state, agent_id);
}

/// Take the executing agent's legacy coucous, stamping `owner` on unscoped ones.
fn drain_legacy(state: &mut cp_base::state::runtime::State, owner: Option<&str>) -> Vec<cp_mod_spine::coucou::Record> {
    let mut list = std::mem::take(&mut SpineState::get_mut(state).legacy_coucous);
    for coucou in &mut list {
        if coucou.agent_id.is_none() {
            coucou.agent_id = owner.map(str::to_owned);
        }
    }
    list
}
