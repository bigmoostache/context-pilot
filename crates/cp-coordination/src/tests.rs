//! State-level tests of the group tool rules: a real `State` with three
//! agents (`A1`, `A2`, `A3`), each with a runtime and a spine inbox.

use cp_base::state::runtime::State;
use cp_base::state::runtime::bundle::AgentRuntime;
use cp_base::tools::ToolUse;
use cp_groups::types::{Group, GroupKind, GroupsState};
use cp_mod_agents::types::AgentsState;
use cp_mod_spine::types::{NotificationType, SpineState};

use crate::ops;

/// Three agents, `A1` executing, each with its own runtime + spine inbox.
fn world() -> State {
    let mut state = State::default();
    let mut agents = AgentsState::new();
    let mut groups = GroupsState::new();
    for name in ["one", "two", "three"] {
        let _id = agents.create(&mut groups, name);
    }
    state.set_ext_global(agents);
    state.set_ext_global(groups);
    for id in ["A1", "A2", "A3"] {
        state.agent_store.insert(id.to_owned(), AgentRuntime::default());
        state.agent_store.set_executing(Some(id.to_owned()));
        state.set_ext_agent(SpineState::new());
    }
    state.agent_store.set_executing(Some("A1".to_owned()));
    state
}

/// A tool call with `input`.
fn tool(input: serde_json::Value) -> ToolUse {
    ToolUse::new(String::new(), String::new(), input)
}

/// Add a group of `kind` owned by `owner`; returns its id.
fn add_group(state: &mut State, kind: GroupKind, owner: &str, members: &[&str]) -> String {
    GroupsState::get_mut(state).create(Group {
        name: "g".into(),
        owner_agent_id: owner.into(),
        member_ids: members.iter().map(|m| (*m).to_owned()).collect(),
        kind,
        ..Group::default()
    })
}

/// Notification sources in `agent`'s inbox.
fn inbox(state: &mut State, agent: &str) -> Vec<String> {
    let prev = state.executing_agent_id().map(str::to_owned);
    state.agent_store.set_executing(Some(agent.to_owned()));
    let out = SpineState::get(state).notifications.iter().map(|n| n.source.clone()).collect();
    state.agent_store.set_executing(prev);
    out
}

/// Bind a notification from `source` to the executing agent `A1`.
fn seed(state: &mut State, source: &str) {
    let nid = SpineState::create_notification(state, NotificationType::Custom, source.to_owned(), "x".into());
    SpineState::set_notification_agent(state, &nid, Some("A1".to_owned()));
}

#[test]
fn only_custom_groups_owned_by_caller_change() {
    let mut s = world();
    let coord = add_group(&mut s, GroupKind::Coordination, "A1", &["A1", "A2"]);
    let other = add_group(&mut s, GroupKind::Custom, "A2", &["A2", "A1"]);
    let human = cp_mod_agents::types::msgs::human_group_id(AgentsState::get(&s), "A1").unwrap().to_owned();
    for gid in [&coord, &other, &human] {
        let del = ops::delete(&tool(serde_json::json!({ "group_id": gid })), &mut s);
        assert!(del.is_err(), "{gid} must not be deletable");
    }
    let upd = tool(serde_json::json!({ "group_id": coord, "before_agent_ids": ["A1", "A2"], "name": "x" }));
    assert!(ops::update(&upd, &mut s).unwrap_err().contains("coordination"), "coordination refused");
    assert!(GroupsState::get(&s).find(&coord).is_some(), "coordination group kept");
}

#[test]
fn update_guards_before_and_after_lists() {
    let mut s = world();
    let gid = add_group(&mut s, GroupKind::Custom, "A1", &["A1", "A2"]);
    let run = |st: &mut State, v: serde_json::Value| ops::update(&tool(v), st);
    let stale = run(&mut s, serde_json::json!({ "group_id": gid, "before_agent_ids": ["A1"], "name": "n" }));
    assert!(stale.unwrap_err().contains("does not match"), "stale before refused, even for a rename");
    let empty =
        run(&mut s, serde_json::json!({ "group_id": gid, "before_agent_ids": ["A1", "A2"], "after_agent_ids": [] }));
    assert!(empty.unwrap_err().contains("empty"), "empty after refused");
    let no_owner = run(
        &mut s,
        serde_json::json!({ "group_id": gid, "before_agent_ids": ["A1", "A2"], "after_agent_ids": ["A2"] }),
    );
    assert!(no_owner.is_err(), "owner must stay");
    let bad = run(&mut s, serde_json::json!({ "group_id": gid, "before_agent_ids": ["A1", 2i32], "name": "n" }));
    assert!(bad.is_err(), "non-string id refused");
    assert_eq!(GroupsState::get(&s).find(&gid).unwrap().member_ids, ["A1", "A2"]);
}

#[test]
fn update_notifies_added_and_removed_members() {
    let mut s = world();
    let gid = add_group(&mut s, GroupKind::Custom, "A1", &["A1", "A2"]);
    let v = serde_json::json!({ "group_id": gid, "before_agent_ids": ["A2", "A1"], "after_agent_ids": ["A1", "A3"] });
    let _ok = ops::update(&tool(v), &mut s).unwrap();
    assert_eq!(inbox(&mut s, "A3").len(), 1, "added member told");
    assert_eq!(inbox(&mut s, "A2").len(), 1, "removed member told");
    assert!(inbox(&mut s, "A1").is_empty(), "owner not notified");
}

#[test]
fn delete_closes_panels_and_notifies_members() {
    let mut s = world();
    let gid = add_group(&mut s, GroupKind::Custom, "A1", &["A1", "A2"]);
    s.agent_store.set_executing(Some("A2".to_owned()));
    let _p2 = cp_groups::panels::open_group_panel(&mut s, &gid);
    s.agent_store.set_executing(Some("A1".to_owned()));
    let _p1 = cp_groups::panels::open_group_panel(&mut s, &gid);
    let _ok = ops::delete(&tool(serde_json::json!({ "group_id": gid })), &mut s).unwrap();
    for id in ["A1", "A2"] {
        let rt = s.agent_store.get(id).unwrap();
        assert!(!rt.context.iter().any(|c| c.context_type.as_str() == "group"), "{id}'s panel closed");
    }
    assert_eq!(inbox(&mut s, "A2").len(), 1, "member told");
    assert!(GroupsState::get(&s).find(&gid).is_none(), "group gone");
}

#[test]
fn read_requires_membership() {
    let mut s = world();
    let gid = add_group(&mut s, GroupKind::Custom, "A2", &["A2", "A3"]);
    let err = ops::read(&tool(serde_json::json!({ "group_id": gid })), &mut s).unwrap_err();
    assert!(err.contains("not a member"), "non-member refused");
}

#[test]
fn read_clears_only_what_it_answers() {
    let mut s = world();
    let gid = add_group(&mut s, GroupKind::Custom, "A2", &["A2", "A1"]);
    seed(&mut s, &format!("social:{gid}:A1:1"));
    seed(&mut s, "coucou:7");
    seed(&mut s, "social:G999:A1:1");
    seed(&mut s, "agents");
    let _first = ops::read(&tool(serde_json::json!({ "group_id": gid })), &mut s).unwrap();
    assert_eq!(inbox(&mut s, "A1"), ["coucou:7", "social:G999:A1:1", "agents"], "turn not held here: nudge kept");
    cp_groups::social::give_turn(GroupsState::get_mut(&mut s).find_mut(&gid).unwrap(), "A1");
    let _second = ops::read(&tool(serde_json::json!({ "group_id": gid })), &mut s).unwrap();
    assert_eq!(inbox(&mut s, "A1"), ["coucou:7", "social:G999:A1:1"], "turn held here: nudge cleared");
}
