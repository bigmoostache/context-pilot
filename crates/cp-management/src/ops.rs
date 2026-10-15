//! `Agent` tool operations: create, read, update, delete sub-agents.
//!
//! Rules (design doc, phase E): the caller is always the executing agent; it
//! creates and updates only its direct children, deletes only its strict
//! descendants, and reads any agent. Creation and deletion that need the main
//! loop are queued in [`ManagementState`].

use std::fmt::Write as _;

use cp_base::state::runtime::State;
use cp_base::tools::ToolUse;
use cp_fleet::AgentExecState;
use cp_groups::chat::{GroupAuthor, GroupMessage};
use cp_groups::social::{self, Next};
use cp_groups::types::{Group, GroupKind, GroupsState};
use cp_mod_agents::types::roster::yaml_escape;
use cp_mod_agents::types::{Agent, AgentsState, msgs, tree, turn};
use cp_mod_agents::view_state::FleetExecMirror;

use crate::keys;
use crate::types::ManagementState;

/// Optional `Agent` tool arguments (trimmed, empty = absent).
struct Args<'tool> {
    /// Target agent.
    id: Option<&'tool str>,
    /// New name (create, update).
    name: Option<&'tool str>,
    /// New description (create, update).
    description: Option<&'tool str>,
    /// Delete confirmation key.
    key: Option<&'tool str>,
    /// First message to the new child (create only).
    message: Option<&'tool str>,
}

/// String argument `key` of `tool`, trimmed; `None` when absent or blank.
fn arg<'tool>(tool: &'tool ToolUse, key: &str) -> Option<&'tool str> {
    tool.input.get(key).and_then(serde_json::Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

/// Run the `Agent` tool for the executing agent.
///
/// # Errors
///
/// Unknown operation, missing argument, or a rule violation; nothing is
/// changed then.
pub fn execute(tool: &ToolUse, state: &mut State) -> Result<String, String> {
    let caller = state.executing_agent_id().unwrap_or_default().to_owned();
    if !AgentsState::get(state).agents.iter().any(|a| a.id == caller) {
        return Err(format!("Agent has no owning agent (executing agent '{caller}' not found)"));
    }
    let args = Args {
        id: arg(tool, "id"),
        name: arg(tool, "name"),
        description: arg(tool, "description"),
        key: arg(tool, "key"),
        message: arg(tool, "message"),
    };
    let operation = arg(tool, "operation").unwrap_or_default();
    if args.message.is_some() && operation != "create" {
        return Err(format!("message is only accepted by create (got operation '{operation}')"));
    }
    match operation {
        "create" => create(state, &caller, &args),
        "read" => read(state, args.id.unwrap_or(&caller)),
        "update" => update(state, &caller, &args),
        "delete" => delete(state, &caller, &args),
        other => Err(format!("unknown operation '{other}': expected create | read | update | delete")),
    }
}

/// Create a direct child of `caller` plus their 1:1 coordination group.
fn create(state: &mut State, caller: &str, args: &Args<'_>) -> Result<String, String> {
    if let Some(id) = args.id {
        return Err(format!("create takes no id (got {id}): ids are assigned"));
    }
    let name = args.name.ok_or_else(|| "create requires a name".to_owned())?;
    let max = ManagementState::get(state).max_depth;
    let caller_depth = tree::depth(&AgentsState::get(state).agents, caller);
    if caller_depth >= max {
        return Err(format!("sub-agent depth is capped at {max}: {caller} is at depth {caller_depth}"));
    }
    let id = msgs::create_agent(state, name);
    if let Some(a) = AgentsState::get_mut(state).agents.iter_mut().find(|a| a.id == id) {
        a.parent_agent_id = Some(caller.to_owned());
        args.description.unwrap_or_default().clone_into(&mut a.description);
    }
    let gid = GroupsState::get_mut(state).create(Group {
        name: format!("{caller} / {id}"),
        owner_agent_id: caller.to_owned(),
        member_ids: vec![caller.to_owned(), id.clone()],
        kind: GroupKind::Coordination,
        ..Group::default()
    });
    ManagementState::get_mut(state).created.push(id.clone());
    if let Some(text) = args.message {
        brief(state, (caller, &id, &gid), text)?;
        return Ok(format!(
            "Created {id} \"{name}\" under {caller}. Coordination group: {gid}. Your message was sent there \
             and {id} holds the turn (next=GIVE_TALKING_STICK_TO {id})."
        ));
    }
    Ok(format!(
        "Created {id} \"{name}\" under {caller}. Coordination group: {gid}. {id} stays idle until you \
         Send in {gid} with next=GIVE_TALKING_STICK_TO {id}."
    ))
}

/// Post `text` from `caller` into the new coordination group `gid` and hand
/// the turn to the child, exactly as `Send(next=GIVE_TALKING_STICK_TO child)`.
fn brief(state: &mut State, (caller, child, gid): (&str, &str, &str), text: &str) -> Result<(), String> {
    let group = GroupsState::get_mut(state).find_mut(gid).ok_or_else(|| format!("group {gid} not found"))?;
    social::apply_send(group, caller, Next::GiveTalkingStickTo(child.to_owned()))?;
    group.messages.push(GroupMessage {
        author: GroupAuthor::Assistant,
        author_agent_id: Some(caller.to_owned()),
        content: Some(text.to_owned()),
        timestamp: cp_base::panels::now_ms(),
        ..GroupMessage::default()
    });
    group.mark_read(caller);
    Ok(())
}

/// Rename / redescribe a direct child of `caller`.
fn update(state: &mut State, caller: &str, args: &Args<'_>) -> Result<String, String> {
    let id = args.id.ok_or_else(|| "update requires an id".to_owned())?;
    if args.name.is_none() && args.description.is_none() {
        return Err("update needs a name and/or a description".to_owned());
    }
    let agents = &mut AgentsState::get_mut(state).agents;
    let agent = agents.iter_mut().find(|a| a.id == id).ok_or_else(|| format!("agent {id} not found"))?;
    if agent.parent_agent_id.as_deref() != Some(caller) {
        return Err(format!("{id} is not a direct child of {caller}"));
    }
    if let Some(n) = args.name {
        n.clone_into(&mut agent.name);
    }
    if let Some(d) = args.description {
        d.clone_into(&mut agent.description);
    }
    Ok(format!("Updated {id} \"{}\": {}", agent.name, agent.description))
}

/// Delete a strict descendant of `caller` with its whole subtree. A subtree
/// of more than one agent needs a confirmation key (see [`keys`]).
fn delete(state: &mut State, caller: &str, args: &Args<'_>) -> Result<String, String> {
    let id = args.id.ok_or_else(|| "delete requires an id".to_owned())?;
    let agents = &AgentsState::get(state).agents;
    if !agents.iter().any(|a| a.id == id) {
        return Err(format!("agent {id} not found"));
    }
    if id == caller || !tree::is_ancestor_or_self(agents, caller, id) {
        return Err(format!("{id} is not a sub-agent of {caller}: you may only delete your descendants"));
    }
    let mut doomed = Vec::new();
    subtree_post_order(agents, id, &mut doomed, agents.len());
    let exec = FleetExecMirror::get(state);
    let busy: Vec<&str> = doomed.iter().map(String::as_str).filter(|d| is_busy(exec.exec_state_of(d))).collect();
    if doomed.len() > 1 || !busy.is_empty() {
        confirm(agents, (caller, id), &busy, args.key)?;
    }
    let gone = doomed.join(", ");
    // Groups they own and coordination groups they sat in go too (files
    // included); they leave every other group.
    let removed =
        social::departed::purge(&mut GroupsState::get_mut(state).groups, &|m: &str| !doomed.iter().any(|d| d == m));
    remove_group_files_off_loop(&removed);
    ManagementState::get_mut(state).doomed.extend(doomed);
    Ok(format!("Deleted {gone} with the groups they owned or coordinated in: {}.", removed.join(", ")))
}

/// Whether an agent in state `s` is mid-work (stream, LLM wait, tool call).
const fn is_busy(s: AgentExecState) -> bool {
    matches!(s, AgentExecState::Streaming | AgentExecState::AwaitingLlm | AgentExecState::AwaitingTool)
}

/// Delete the files of `removed` groups on a detached thread, so the tool
/// never blocks the main loop on disk I/O. Inline only if the spawn fails.
fn remove_group_files_off_loop(removed: &[String]) {
    if removed.is_empty() {
        return;
    }
    let bg = removed.to_vec();
    let spawned = std::thread::Builder::new().name("group-file-rm".into()).spawn(move || {
        for gid in &bg {
            cp_groups::persist::remove_group_file(gid);
        }
    });
    if spawned.is_err() {
        for gid in removed {
            cp_groups::persist::remove_group_file(gid);
        }
    }
}

/// Spend a `key` issued for this `(caller, id)` delete, or refuse with the
/// subtree listing, the busy agents and a fresh key bound to this delete.
fn confirm(agents: &[Agent], (caller, id): (&str, &str), busy: &[&str], key: Option<&str>) -> Result<(), String> {
    let now = cp_base::panels::now_ms();
    if key.is_some_and(|k| keys::consume(k, caller, id, now)) {
        return Ok(());
    }
    let mut listing = String::new();
    write_subtree(agents, id, 0, &mut listing);
    let invalid = if key.is_some() { "Invalid or expired key for this delete. " } else { "" };
    let busy_line = if busy.is_empty() {
        String::new()
    } else {
        format!("Busy right now (streaming, waiting on the LLM or in a tool call): {}.\n", busy.join(", "))
    };
    Err(format!(
        "{invalid}Deleting {id} needs confirmation. It would delete:\n{listing}{busy_line}\
         If you are absolutely sure, call again with key=\"{}\" (valid 5 minutes, for this delete only).",
        keys::issue(caller, id, now)
    ))
}

/// Push `id`'s subtree, children before parents; `budget` bounds the recursion.
fn subtree_post_order(agents: &[Agent], id: &str, out: &mut Vec<String>, budget: usize) {
    if let Some(left) = budget.checked_sub(1) {
        for c in tree::children(agents, id) {
            subtree_post_order(agents, &c.id, out, left);
        }
    }
    out.push(id.to_owned());
}

/// Indented `- id "name": description` lines for `id` and its descendants.
fn write_subtree(agents: &[Agent], id: &str, depth: usize, out: &mut String) {
    let Some(a) = agents.iter().find(|a| a.id == id) else { return };
    _ = writeln!(out, "{}- {} \"{}\": {}", "  ".repeat(depth), a.id, a.name, a.description);
    if depth < agents.len() {
        for c in tree::children(agents, id) {
            write_subtree(agents, &c.id, depth.saturating_add(1), out);
        }
    }
}

/// Snake-case label of an execution state.
const fn exec_label(s: AgentExecState) -> &'static str {
    match s {
        AgentExecState::Idle => "idle",
        AgentExecState::Runnable => "runnable",
        AgentExecState::Streaming => "streaming",
        AgentExecState::Errored => "errored",
    }
}

/// `(open panels, context tokens)` of `id`'s runtime, when it has one.
fn runtime_stats(state: &State, id: &str) -> Option<(usize, usize)> {
    let rt = if state.executing_agent_id() == Some(id) { Some(state.agent()) } else { state.agent_store.get(id) }?;
    Some((rt.context.len(), rt.context.iter().map(|c| c.token_count).sum()))
}

/// YAML stats of agent `id`: identity, tree links, turn, execution, groups,
/// messages and context usage.
fn read(state: &State, id: &str) -> Result<String, String> {
    let agents = &AgentsState::get(state).agents;
    let a = agents.iter().find(|a| a.id == id).ok_or_else(|| format!("agent {id} not found"))?;
    let children: Vec<&str> = tree::children(agents, id).iter().map(|c| c.id.as_str()).collect();
    let groups: Vec<&str> = GroupsState::get(state).of_member(id).map(|g| g.id.as_str()).collect();
    let mut out = format!("agent:\n  id: {id}\n  name: \"{}\"\n", yaml_escape(&a.name));
    _ = writeln!(out, "  description: \"{}\"", yaml_escape(&a.description));
    _ = writeln!(out, "  parent: {}", a.parent_agent_id.as_deref().unwrap_or("none"));
    _ = writeln!(out, "  children: [{}]", children.join(", "));
    _ = writeln!(out, "  turn: {}", turn::status(state, id));
    _ = writeln!(out, "  exec: {}", exec_label(FleetExecMirror::get(state).exec_state_of(id)));
    _ = writeln!(out, "  archived: {}\n  paused: {}", a.archived, a.paused);
    _ = writeln!(out, "  messages: {}", msgs::messages(state, id).len());
    _ = writeln!(out, "  groups: [{}]", groups.join(", "));
    if let Some((panels, tokens)) = runtime_stats(state, id) {
        _ = writeln!(out, "  panels_open: {panels}\n  context_tokens: {tokens}");
    }
    Ok(out)
}
