//! `Group_*` tool operations. The caller is always the executing agent.

use cp_base::state::runtime::State;
use cp_base::tools::ToolUse;
use cp_groups::social;
use cp_groups::types::{Group, GroupKind, GroupsState};
use cp_mod_agents::types::AgentsState;

use crate::effects;

/// String argument `key`, trimmed; `None` when absent or blank.
fn arg<'tool>(tool: &'tool ToolUse, key: &str) -> Option<&'tool str> {
    tool.input.get(key).and_then(serde_json::Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

/// Agent-id list argument `key`: a JSON array of strings, or one string
/// separated by commas/spaces. Trimmed, blanks dropped, order kept, deduped.
/// `Ok(None)` when absent.
///
/// # Errors
///
/// A non-string array item, or a value that is neither array nor string.
fn ids(tool: &ToolUse, key: &str) -> Result<Option<Vec<String>>, String> {
    let Some(value) = tool.input.get(key) else { return Ok(None) };
    let raw: Vec<&str> = if let Some(items) = value.as_array() {
        items
            .iter()
            .map(|v| v.as_str().ok_or_else(|| format!("{key} must hold agent id strings, got {v}")))
            .collect::<Result<_, _>>()?
    } else {
        value.as_str().ok_or_else(|| format!("{key} must be an array of agent ids"))?.split([',', ' ']).collect()
    };
    let mut out: Vec<String> = Vec::new();
    for id in raw.into_iter().map(str::trim).filter(|s| !s.is_empty()) {
        if !out.iter().any(|o| o == id) {
            out.push(id.to_owned());
        }
    }
    Ok(Some(out))
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

/// The group `gid`, which `caller` must own. Human and coordination groups
/// are refused: both live and die with their agent.
fn owned<'state>(state: &'state mut State, gid: &str, caller: &str) -> Result<&'state mut Group, String> {
    let group = GroupsState::get_mut(state).find_mut(gid).ok_or_else(|| format!("group {gid} not found"))?;
    if group.owner_agent_id != caller {
        return Err(format!("only the owner ({}) may change {gid}; ask them", group.owner_agent_id));
    }
    match group.kind {
        GroupKind::Human => Err(format!("{gid} is a human group: it cannot be changed or deleted")),
        GroupKind::Coordination => Err(format!(
            "{gid} is a sub-agent's coordination group: it lives and dies with the sub-agent \
             (Agent delete), and cannot be changed"
        )),
        GroupKind::Custom => Ok(group),
    }
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
    let mut members = ids(tool, "agent_ids")?.unwrap_or_default();
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

/// `Read(group_id)`: open the group's panel, or refresh it; members only.
///
/// Marks the group's messages read and clears the caller's spine
/// notifications this read answers (see [`effects::clear_read`]);
/// notifications from elsewhere stay.
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
    let unread = group.unread_for(&me);
    group.mark_read(&me);
    let human = group.kind == GroupKind::Human;
    if human {
        for m in &mut group.messages {
            m.acknowledged = true;
        }
    }
    let holds_turn = group.social.iter().any(|s| s.holds(&me));
    let _cleared = effects::clear_read(state, &effects::ReadScope { me: &me, gid, human, holds_turn });
    let panel = cp_groups::panels::open_group_panel(state, gid);
    Ok(format!("{gid} \"{name}\" is fresh in panel {panel} ({unread} new message(s))."))
}

/// The `after_agent_ids` list, when given: non-empty, known agents, and
/// still holding the owner `me`.
fn valid_after(state: &State, tool: &ToolUse, me: &str) -> Result<Option<Vec<String>>, String> {
    let Some(after) = ids(tool, "after_agent_ids")? else { return Ok(None) };
    if after.is_empty() {
        return Err("after_agent_ids is empty: it must include you, the owner (delete the group instead)".to_owned());
    }
    if !after.iter().any(|a| a == me) {
        return Err("after_agent_ids must include you, the owner (delete the group instead)".to_owned());
    }
    check_agents(state, &after)?;
    Ok(Some(after))
}

/// `Group_update`: change a group the caller owns.
///
/// Takes `group_id`, `before_agent_ids`, and optional `after_agent_ids`,
/// `name`, `description`. `before_agent_ids` must match the current members
/// exactly (any order), so the owner knows what it changes.
///
/// # Errors
///
/// Not owner, human or coordination group, stale `before`, empty `after`,
/// owner missing from `after`, unknown agents, malformed id lists, or
/// nothing to change.
pub fn update(tool: &ToolUse, state: &mut State) -> Result<String, String> {
    let me = caller(state, "Group_update")?;
    let gid = arg(tool, "group_id").ok_or_else(|| "Group_update requires a group_id".to_owned())?.to_owned();
    let before = ids(tool, "before_agent_ids")?.ok_or_else(|| "Group_update requires before_agent_ids".to_owned())?;
    let after = valid_after(state, tool, &me)?;
    let (name, description) = (arg(tool, "name"), arg(tool, "description"));
    if after.is_none() && name.is_none() && description.is_none() {
        return Err("Group_update needs after_agent_ids, name and/or description".to_owned());
    }
    let group = owned(state, &gid, &me)?;
    if !same_set(&before, &group.member_ids) {
        return Err(format!("before_agent_ids does not match {gid}'s members: [{}]", group.member_ids.join(", ")));
    }
    let (mut added, mut removed) = (Vec::new(), Vec::new());
    if let Some(a) = after {
        let old = std::mem::replace(&mut group.member_ids, a.clone());
        removed = old.iter().filter(|m| !a.contains(m)).cloned().collect();
        added = a.into_iter().filter(|m| !old.contains(m)).collect();
        for gone in &removed {
            social::departed::forget_member(group, gone);
        }
    }
    if let Some(n) = name {
        n.clone_into(&mut group.name);
    }
    if let Some(d) = description {
        d.clone_into(&mut group.description);
    }
    let (label, members) = (group.name.clone(), group.member_ids.join(", "));
    for id in &added {
        effects::notify(
            state,
            &gid,
            id,
            &format!("{me} added you to {gid} \"{label}\". Read(group_id={gid}) to see it."),
        );
    }
    for id in &removed {
        effects::notify(state, &gid, id, &format!("{me} removed you from {gid} \"{label}\"."));
    }
    effects::close_panels_of(state, &gid, &removed);
    Ok(format!(
        "Updated {gid} \"{label}\": members [{members}]. Notified: +[{}] -[{}].",
        added.join(", "),
        removed.join(", ")
    ))
}

/// `Group_delete(group_id)`: owner only, custom groups only. Every member's
/// panel on it closes, the other members are told, and its messages file is
/// deleted off the main loop.
///
/// # Errors
///
/// Unknown group, not owner, or a human / coordination group.
pub fn delete(tool: &ToolUse, state: &mut State) -> Result<String, String> {
    let me = caller(state, "Group_delete")?;
    let gid = arg(tool, "group_id").ok_or_else(|| "Group_delete requires a group_id".to_owned())?.to_owned();
    let group = owned(state, &gid, &me)?;
    let (label, members) = (group.name.clone(), group.member_ids.clone());
    GroupsState::get_mut(state).groups.retain(|g| g.id != gid);
    effects::close_panels_of(state, &gid, &members);
    for id in members.iter().filter(|m| **m != me) {
        effects::notify(state, &gid, id, &format!("{me} deleted group {gid} \"{label}\"."));
    }
    effects::remove_file_off_loop(&gid);
    Ok(format!("Deleted {gid}."))
}

/// Same members, any order (both lists are deduped).
fn same_set(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().all(|x| b.contains(x))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_accepts_array_and_string() {
        let t = |v: serde_json::Value| ToolUse::new(String::new(), String::new(), serde_json::json!({ "x": v }));
        assert_eq!(ids(&t(serde_json::json!(["A1", " A2", "A1"])), "x"), Ok(Some(vec!["A1".into(), "A2".into()])));
        assert_eq!(ids(&t(serde_json::json!("A1, A2 A3")), "x"), Ok(Some(vec!["A1".into(), "A2".into(), "A3".into()])));
        assert!(ids(&t(serde_json::json!(["A1", 2i32])), "x").is_err(), "non-string item rejected");
        assert!(ids(&t(serde_json::json!(7i32)), "x").is_err(), "non-list value rejected");
    }

    #[test]
    fn same_set_ignores_order() {
        let v = |s: &[&str]| s.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
        assert!(same_set(&v(&["A1", "A2"]), &v(&["A2", "A1"])), "order free");
        assert!(!same_set(&v(&["A1"]), &v(&["A1", "A2"])), "missing member");
    }
}
