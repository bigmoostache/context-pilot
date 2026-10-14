//! `Group_*` tool operations. The caller is always the executing agent.

use cp_base::state::runtime::State;
use cp_base::tools::ToolUse;
use cp_groups::social::{self, Substate};
use cp_groups::types::{Group, GroupKind, GroupsState};
use cp_mod_agents::types::AgentsState;

/// String argument `key`, trimmed; `None` when absent or blank.
fn arg<'tool>(tool: &'tool ToolUse, key: &str) -> Option<&'tool str> {
    tool.input.get(key).and_then(serde_json::Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

/// Agent-id list argument `key`: a JSON array of strings, or one string
/// separated by commas/spaces. Trimmed, blanks dropped, order kept, deduped.
fn ids(tool: &ToolUse, key: &str) -> Option<Vec<String>> {
    let value = tool.input.get(key)?;
    let raw: Vec<&str> = if let Some(items) = value.as_array() {
        items.iter().filter_map(serde_json::Value::as_str).collect()
    } else {
        value.as_str()?.split([',', ' ']).collect()
    };
    let mut out: Vec<String> = Vec::new();
    for id in raw.into_iter().map(str::trim).filter(|s| !s.is_empty()) {
        if !out.iter().any(|o| o == id) {
            out.push(id.to_owned());
        }
    }
    Some(out)
}

/// The executing agent, which must exist.
fn caller(state: &State, tool: &str) -> Result<String, String> {
    let id = state.executing_agent_id().unwrap_or_default().to_owned();
    if AgentsState::get(state).agents.iter().any(|a| a.id == id) {
        Ok(id)
    } else {
        Err(format!("{tool} has no owning agent (executing agent '{id}' not found)"))
    }
}

/// Every id must name an existing agent (the human talks in human groups only).
fn check_agents(state: &State, list: &[String]) -> Result<(), String> {
    let agents = &AgentsState::get(state).agents;
    let unknown: Vec<&str> =
        list.iter().filter(|id| !agents.iter().any(|a| a.id == **id)).map(String::as_str).collect();
    if unknown.is_empty() { Ok(()) } else { Err(format!("unknown agent(s): {}", unknown.join(", "))) }
}

/// The group `gid`, which `caller` must own; human groups are refused.
fn owned<'state>(state: &'state mut State, gid: &str, caller: &str) -> Result<&'state mut Group, String> {
    let group = GroupsState::get_mut(state).find_mut(gid).ok_or_else(|| format!("group {gid} not found"))?;
    if group.owner_agent_id != caller {
        return Err(format!("only the owner ({}) may change {gid}; ask them", group.owner_agent_id));
    }
    if group.kind == GroupKind::Human {
        return Err(format!("{gid} is a human group: it cannot be changed or deleted"));
    }
    Ok(group)
}

/// `Group_create(agent_ids, name, description?)`: a custom group owned by
/// the caller, who is always a member. Opens its panel for the caller.
///
/// # Errors
///
/// Missing name, empty or unknown members.
pub fn create(tool: &ToolUse, state: &mut State) -> Result<String, String> {
    let me = caller(state, "Group_create")?;
    let name = arg(tool, "name").ok_or_else(|| "Group_create requires a name".to_owned())?;
    let mut members = ids(tool, "agent_ids").unwrap_or_default();
    members.retain(|m| *m != me);
    if members.is_empty() {
        return Err("Group_create needs at least one other agent in agent_ids".to_owned());
    }
    check_agents(state, &members)?;
    members.insert(0, me.clone());
    let gid = GroupsState::get_mut(state).create(Group {
        name: name.to_owned(),
        description: arg(tool, "description").unwrap_or_default().to_owned(),
        owner_agent_id: me,
        member_ids: members.clone(),
        kind: GroupKind::Custom,
        ..Group::default()
    });
    let panel = cp_groups::panels::open_group_panel(state, &gid);
    Ok(format!("Created {gid} \"{name}\" with [{}], panel {panel}.", members.join(", ")))
}

/// `Read(group_id)`: open the group's panel, or refresh it when already
/// open; members only. Marks the group's messages read and clears the
/// caller's spine notifications for its human group.
///
/// # Errors
///
/// Unknown group, or the caller is not a member.
pub fn read(tool: &ToolUse, state: &mut State) -> Result<String, String> {
    let me = caller(state, "Read")?;
    let gid = arg(tool, "group_id").ok_or_else(|| "Read requires a group_id".to_owned())?;
    let group = GroupsState::get_mut(state).find_mut(gid).ok_or_else(|| format!("group {gid} not found"))?;
    if !group.member_ids.contains(&me) {
        return Err(format!("you ({me}) are not a member of {gid}"));
    }
    let name = group.name.clone();
    let unread = group.messages.iter().filter(|m| !m.acknowledged).count();
    for m in &mut group.messages {
        m.acknowledged = true;
    }
    if group.kind == GroupKind::Human {
        let _cleared = cp_mod_spine::types::SpineState::delete_notifications_by_agent(state, &me);
    }
    let panel = cp_groups::panels::open_group_panel(state, gid);
    Ok(format!("{gid} \"{name}\" is fresh in panel {panel} ({unread} new message(s))."))
}

/// `Group_update`: change a group the caller owns.
///
/// Takes `group_id`, `before_agent_ids`, and optional `after_agent_ids`,
/// `name`, `description`. `before_agent_ids` must match the current members
/// exactly (any order), so the owner knows what it changes.
///
/// # Errors
///
/// Not owner, human group, stale `before`, owner missing from `after`,
/// unknown agents, or nothing to change.
pub fn update(tool: &ToolUse, state: &mut State) -> Result<String, String> {
    let me = caller(state, "Group_update")?;
    let gid = arg(tool, "group_id").ok_or_else(|| "Group_update requires a group_id".to_owned())?;
    let before = ids(tool, "before_agent_ids").ok_or_else(|| "Group_update requires before_agent_ids".to_owned())?;
    let after = ids(tool, "after_agent_ids").filter(|a| !a.is_empty());
    let (name, description) = (arg(tool, "name"), arg(tool, "description"));
    if after.is_none() && name.is_none() && description.is_none() {
        return Err("Group_update needs after_agent_ids, name and/or description".to_owned());
    }
    if let Some(a) = after.as_ref() {
        check_agents(state, a)?;
        if !a.contains(&me) {
            return Err("after_agent_ids must include you, the owner (delete the group instead)".to_owned());
        }
    }
    let group = owned(state, gid, &me)?;
    if !same_set(&before, &group.member_ids) {
        return Err(format!("before_agent_ids does not match {gid}'s members: [{}]", group.member_ids.join(", ")));
    }
    if let Some(a) = after {
        let old = std::mem::replace(&mut group.member_ids, a.clone());
        for gone in old.iter().filter(|m| !a.contains(m)) {
            forget_member(group, gone);
        }
    }
    if let Some(n) = name {
        n.clone_into(&mut group.name);
    }
    if let Some(d) = description {
        d.clone_into(&mut group.description);
    }
    Ok(format!("Updated {gid} \"{}\": members [{}].", group.name, group.member_ids.join(", ")))
}

/// `Group_delete(group_id)`: owner only, never a human group. The messages
/// file stays on disk as a backup.
///
/// # Errors
///
/// Unknown group, not owner, or human group.
pub fn delete(tool: &ToolUse, state: &mut State) -> Result<String, String> {
    let me = caller(state, "Group_delete")?;
    let gid = arg(tool, "group_id").ok_or_else(|| "Group_delete requires a group_id".to_owned())?.to_owned();
    let _group = owned(state, &gid, &me)?;
    GroupsState::get_mut(state).groups.retain(|g| g.id != gid);
    Ok(format!("Deleted {gid}."))
}

/// Same members, any order (both lists are deduped).
fn same_set(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().all(|x| b.contains(x))
}

/// Drop every social obligation naming `agent_id` (it left the group). A
/// dispatch left with no candidate is dropped: empty would mean "everyone".
fn forget_member(group: &mut Group, agent_id: &str) {
    social::clear_turn(group, agent_id);
    group.social = std::mem::take(&mut group.social).into_iter().filter_map(|s| keep_without(s, agent_id)).collect();
}

/// `s` with `gone` removed, or `None` when nothing is left of it.
fn keep_without(s: Substate, gone: &str) -> Option<Substate> {
    match s {
        Substate::ComingBack { agent_id, at_ms, message } => {
            (agent_id != gone).then_some(Substate::ComingBack { agent_id, at_ms, message })
        }
        Substate::Dispatch { mut candidates } => {
            let had = !candidates.is_empty();
            candidates.retain(|c| c != gone);
            (!had || !candidates.is_empty()).then_some(Substate::Dispatch { candidates })
        }
        other @ (Substate::SimpleTurn { .. } | Substate::Gather { .. }) => Some(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_accepts_array_and_string() {
        let t = |v: serde_json::Value| ToolUse::new(String::new(), String::new(), serde_json::json!({ "x": v }));
        assert_eq!(ids(&t(serde_json::json!(["A1", " A2", "A1"])), "x"), Some(vec!["A1".into(), "A2".into()]));
        assert_eq!(ids(&t(serde_json::json!("A1, A2 A3")), "x"), Some(vec!["A1".into(), "A2".into(), "A3".into()]));
    }

    #[test]
    fn same_set_ignores_order() {
        let v = |s: &[&str]| s.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
        assert!(same_set(&v(&["A1", "A2"]), &v(&["A2", "A1"])), "order free");
        assert!(!same_set(&v(&["A1"]), &v(&["A1", "A2"])), "missing member");
    }

    #[test]
    fn forget_member_prunes_obligations() {
        let mut g = Group {
            social: vec![
                Substate::SimpleTurn { agent_id: "A2".into() },
                Substate::ComingBack { agent_id: "A2".into(), at_ms: 1, message: String::new() },
                Substate::Dispatch { candidates: vec!["A2".into()] },
                Substate::Dispatch { candidates: vec!["A2".into(), "A3".into()] },
            ],
            ..Group::default()
        };
        forget_member(&mut g, "A2");
        assert_eq!(g.social, vec![Substate::Dispatch { candidates: vec!["A3".into()] }]);
    }
}
