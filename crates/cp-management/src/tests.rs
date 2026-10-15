//! `Agent` tool rules: depth cap, ownership, delete confirmation and busy guard.

use std::collections::HashMap;

use cp_base::state::runtime::State;
use cp_base::tools::ToolUse;
use cp_fleet::AgentExecState;
use cp_groups::types::GroupsState;
use cp_mod_agents::types::{AgentsState, msgs};
use cp_mod_agents::view_state::FleetExecMirror;
use serde_json::{Value, json};

use crate::ops::execute;
use crate::types::ManagementState;

/// A state holding one root agent `A1`, executing.
fn fixture() -> State {
    let mut state = State::default();
    state.set_ext(AgentsState::new());
    state.set_ext(GroupsState::new());
    state.set_ext(ManagementState::new());
    state.set_ext(FleetExecMirror::new());
    let root = msgs::create_agent(&mut state, "root");
    state.agent_store.set_executing(Some(root));
    state
}

/// Run the `Agent` tool as `caller`.
fn run(state: &mut State, caller: &str, input: Value) -> Result<String, String> {
    state.agent_store.set_executing(Some(caller.to_owned()));
    execute(&ToolUse::new("t".to_owned(), "Agent".to_owned(), input), state)
}

/// Create a child of `caller`; returns its id.
fn child(state: &mut State, caller: &str, name: &str) -> String {
    let before = AgentsState::get(state).agents.len();
    let _ok = run(state, caller, json!({"operation": "create", "name": name})).expect("create");
    let agents = &AgentsState::get(state).agents;
    assert_eq!(agents.len(), before.saturating_add(1), "one agent created");
    agents.last().map(|a| a.id.clone()).unwrap_or_default()
}

/// Ids of the agents still in the roster.
fn ids(state: &State) -> Vec<String> {
    AgentsState::get(state).agents.iter().map(|a| a.id.clone()).collect()
}

/// The `key="..."` of a refused delete.
fn key_of(err: &str) -> String {
    err.split("key=\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or_default().to_owned()
}

/// Delete `id` as `caller`, with an optional key.
fn delete(state: &mut State, caller: &str, id: &str, key: Option<&str>) -> Result<String, String> {
    let mut input = json!({"operation": "delete", "id": id});
    if let (Some(k), Some(obj)) = (key, input.as_object_mut()) {
        let _prev = obj.insert("key".to_owned(), json!(k));
    }
    run(state, caller, input)
}

#[test]
fn create_links_child_and_coordination_group() {
    let mut s = fixture();
    let c = child(&mut s, "A1", "kid");
    let agents = &AgentsState::get(&s).agents;
    assert_eq!(agents.iter().find(|a| a.id == c).and_then(|a| a.parent_agent_id.clone()), Some("A1".to_owned()));
    assert!(GroupsState::get(&s).of_member(&c).any(|g| g.member_ids.iter().any(|m| m == "A1")), "1:1 group");
    assert_eq!(ManagementState::get(&s).created, vec![c], "queued for the main loop");
}

#[test]
fn create_refused_at_depth_cap() {
    let mut s = fixture();
    ManagementState::get_mut(&mut s).max_depth = 1;
    let c = child(&mut s, "A1", "kid");
    let err = run(&mut s, &c, json!({"operation": "create", "name": "grandkid"})).expect_err("capped");
    assert!(err.contains("depth is capped"), "{err}");
}

#[test]
fn update_only_direct_children() {
    let mut s = fixture();
    let c = child(&mut s, "A1", "kid");
    let grand = child(&mut s, &c, "grandkid");
    assert!(run(&mut s, "A1", json!({"operation": "update", "id": grand, "name": "x"})).is_err(), "grandchild");
    assert!(run(&mut s, "A1", json!({"operation": "update", "id": c, "name": "renamed"})).is_ok(), "child");
}

#[test]
fn delete_refuses_self_and_non_descendants() {
    let mut s = fixture();
    let other = msgs::create_agent(&mut s, "other");
    assert!(delete(&mut s, "A1", "A1", None).is_err(), "self");
    assert!(delete(&mut s, "A1", &other, None).is_err(), "not a descendant");
    assert_eq!(ids(&s).len(), 2, "nothing deleted");
}

#[test]
fn idle_leaf_delete_needs_no_key_and_purges_its_group() {
    let mut s = fixture();
    let c = child(&mut s, "A1", "kid");
    let _ok = delete(&mut s, "A1", &c, None).expect("leaf delete");
    assert!(GroupsState::get(&s).of_member(&c).next().is_none(), "coordination group purged");
    assert_eq!(ManagementState::get(&s).doomed, vec![c]);
}

#[test]
fn subtree_delete_refused_without_key_changes_nothing() {
    let mut s = fixture();
    let c = child(&mut s, "A1", "kid");
    let _g = child(&mut s, &c, "grandkid");
    let groups_before = GroupsState::get(&s).groups.len();
    let err = delete(&mut s, "A1", &c, None).expect_err("needs key");
    assert!(!key_of(&err).is_empty(), "{err}");
    assert_eq!(GroupsState::get(&s).groups.len(), groups_before, "groups untouched");
    assert!(ManagementState::get(&s).doomed.is_empty(), "nothing queued");
}

#[test]
fn key_confirms_only_the_delete_it_was_issued_for() {
    let mut s = fixture();
    let first = child(&mut s, "A1", "a");
    let _a2 = child(&mut s, &first, "a2");
    let second = child(&mut s, "A1", "b");
    let _b2 = child(&mut s, &second, "b2");
    let key_a = key_of(&delete(&mut s, "A1", &first, None).expect_err("needs key"));
    assert!(delete(&mut s, "A1", &second, Some(&key_a)).is_err(), "a's key refused for b");
    let gone = delete(&mut s, "A1", &first, Some(&key_a)).expect("a's key works for a");
    assert!(gone.contains(&first), "{gone}");
}

#[test]
fn busy_leaf_delete_needs_a_key() {
    let mut s = fixture();
    let c = child(&mut s, "A1", "kid");
    FleetExecMirror::get_mut(&mut s).replace_all(HashMap::from([(c.clone(), AgentExecState::Streaming)]));
    let err = delete(&mut s, "A1", &c, None).expect_err("busy");
    assert!(err.contains("Busy right now"), "{err}");
    let _ok = delete(&mut s, "A1", &c, Some(&key_of(&err))).expect("confirmed");
}
