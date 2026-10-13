//! An agent's conversation lives in its human group (T860 phase C).
//!
//! These accessors resolve `agent id → Agent::human_group_id → Group::messages`
//! so call sites keep reading "the agent's messages" without knowing about
//! groups.

use cp_base::state::runtime::State;
use cp_groups::types::{GroupKind, GroupsState};

use super::{Agent, AgentMessage, AgentsState};

/// Human-group id of `agent_id` (`None` if the agent or its group is unknown).
#[must_use]
pub fn human_group_id<'store>(agents: &'store AgentsState, agent_id: &str) -> Option<&'store str> {
    agents.agents.iter().find(|a| a.id == agent_id).map(|a| a.human_group_id.as_str()).filter(|g| !g.is_empty())
}

/// Messages of `agent_id`'s human group, from explicit stores.
#[must_use]
pub fn of<'store>(agents: &AgentsState, groups: &'store GroupsState, agent_id: &str) -> &'store [AgentMessage] {
    human_group_id(agents, agent_id).and_then(|g| groups.find(g)).map_or(&[], |g| g.messages.as_slice())
}

/// Messages of `agent_id`'s human group (empty when unknown).
#[must_use]
pub fn messages<'store>(state: &'store State, agent_id: &str) -> &'store [AgentMessage] {
    of(AgentsState::get(state), GroupsState::get(state), agent_id)
}

/// Mutable messages of `agent_id`'s human group.
pub fn messages_mut<'store>(state: &'store mut State, agent_id: &str) -> Option<&'store mut Vec<AgentMessage>> {
    let gid = human_group_id(AgentsState::get(state), agent_id)?.to_owned();
    GroupsState::get_mut(state).find_mut(&gid).map(|g| &mut g.messages)
}

/// Run `f` with both stores borrowed mutably (they live in separate `State` slots).
fn with_stores<R>(state: &mut State, f: impl FnOnce(&mut AgentsState, &mut GroupsState) -> R) -> R {
    let mut groups = std::mem::take(GroupsState::get_mut(state));
    let out = f(AgentsState::get_mut(state), &mut groups);
    *GroupsState::get_mut(state) = groups;
    out
}

/// [`AgentsState::create`] on `state`: new agent + its human group; returns its id.
pub fn create_agent(state: &mut State, name: &str) -> String {
    with_stores(state, |agents, groups| agents.create(groups, name))
}

/// [`AgentsState::branch`] on `state`.
///
/// # Errors
///
/// Unknown source agent or branch point.
pub fn branch_agent(state: &mut State, at: (&str, u64), name: &str) -> Result<String, String> {
    with_stores(state, |agents, groups| agents.branch(groups, at, name))
}

/// Give every agent lacking one a human group, seeded with `legacy(agent_id)`
/// (its pre-groups message file). Returns how many groups were created.
pub fn ensure_human_groups<F>(agents: &mut [Agent], groups: &mut GroupsState, legacy: F) -> usize
where
    F: Fn(&str) -> Vec<AgentMessage>,
{
    let mut created = 0usize;
    for agent in agents.iter_mut() {
        let known = groups.find(&agent.human_group_id).is_some_and(|g| g.kind == GroupKind::Human);
        if known {
            continue;
        }
        let gid = groups.create_human_group(&agent.id, &agent.name);
        if let Some(g) = groups.find_mut(&gid) {
            g.messages = legacy(&agent.id);
            g.created_at = agent.created_at;
        }
        agent.human_group_id = gid;
        created = created.saturating_add(1);
    }
    created
}
