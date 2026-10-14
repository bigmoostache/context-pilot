//! An agent's turn is derived from group social state (T860 phase D).
//!
//! There is no stored per-agent `MY_TURN` flag anymore: an agent is
//! [`AgentStatus::MyTurn`] exactly when some substate of some group holds it
//! ([`cp_groups::social::runnable`]). Turn writes go through the agent's human
//! group.

use cp_base::state::runtime::State;
use cp_groups::social;
use cp_groups::types::{Group, GroupsState};

use super::{AgentStatus, AgentsState, msgs};

/// Turn of `agent_id` over explicit `groups`.
#[must_use]
pub fn status_of(groups: &[Group], agent_id: &str) -> AgentStatus {
    if social::runnable(groups, agent_id) { AgentStatus::MyTurn } else { AgentStatus::TheirTurn }
}

/// Turn of `agent_id`, read from `state`'s groups.
#[must_use]
pub fn status(state: &State, agent_id: &str) -> AgentStatus {
    status_of(&GroupsState::get(state).groups, agent_id)
}

/// Ids of the groups whose substates make `agent_id` runnable.
#[must_use]
pub fn holding_groups(groups: &[Group], agent_id: &str) -> Vec<String> {
    groups.iter().filter(|g| g.social.iter().any(|s| s.holds(agent_id))).map(|g| g.id.clone()).collect()
}

/// "Please respond" nudge for a `MY_TURN` agent, naming the groups that hold
/// its turn: a reply in any other group leaves the turn where it is.
#[must_use]
pub fn nudge(state: &State, agent_id: &str, name: &str) -> String {
    let store = GroupsState::get(state);
    let held = holding_groups(&store.groups, agent_id);
    let groups = if held.is_empty() {
        "?".to_owned()
    } else {
        held.iter().map(|g| store.label(g)).collect::<Vec<_>>().join(", ")
    };
    format!(
        "Agent \"{name}\" ({agent_id}) is MY_TURN: you hold the turn in {groups}. \
         Reply there (Send group_id={groups_first}); sending elsewhere does not release it.",
        groups_first = held.first().map_or("?", String::as_str),
    )
}

/// The human group of `agent_id`, mutably.
fn human_group_mut<'store>(state: &'store mut State, agent_id: &str) -> Option<&'store mut Group> {
    let gid = msgs::human_group_id(AgentsState::get(state), agent_id)?.to_owned();
    GroupsState::get_mut(state).find_mut(&gid)
}

/// Give `agent_id` the talking stick in its human group (idempotent).
pub fn give(state: &mut State, agent_id: &str) {
    if let Some(g) = human_group_mut(state, agent_id) {
        social::give_turn(g, agent_id);
    }
}

/// Drop `agent_id`'s obligations in its human group (hand the turn back).
pub fn end(state: &mut State, agent_id: &str) {
    if let Some(g) = human_group_mut(state, agent_id) {
        social::clear_turn(g, agent_id);
    }
}

/// Ids of agents persisted with the pre-groups `"status": "MyTurn"` field.
///
/// Read once from raw `config.json` by the boot migration; the field is no
/// longer written, so the migration cannot fire twice.
#[must_use]
pub fn legacy_my_turn_ids(agents: &serde_json::Value) -> Vec<String> {
    agents
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|a| a.get("status").and_then(serde_json::Value::as_str) == Some("MyTurn"))
                .filter_map(|a| a.get("id").and_then(serde_json::Value::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}
