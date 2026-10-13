//! Unit tests for the agent types (extracted from `types.rs` for the
//! 500-line cap). `super` is the `types` module, so `use super::*` keeps the
//! exact same scope the tests had when inlined.

use cp_groups::types::GroupsState;

use super::*;

/// A message by `author` stamped `ts`, unacknowledged.
fn msg(author: AgentAuthor, text: &str, ts: u64) -> AgentMessage {
    AgentMessage {
        author,
        author_agent_id: None,
        content: Some(text.to_owned()),
        file_path: None,
        timestamp: ts,
        acknowledged: false,
        auto: false,
        has_been_pushed: false,
    }
}

/// One parent agent `A1` with four messages (ts 10..=40) in its human group,
/// archived + paused + `MyTurn` so the test can check none of it carries over.
fn parent_state() -> (AgentsState, GroupsState) {
    let mut ts = AgentsState::new();
    let mut gs = GroupsState::new();
    let id = ts.create(&mut gs, "Parent");
    let gid = msgs::human_group_id(&ts, &id).unwrap().to_owned();
    gs.find_mut(&gid).unwrap().messages = vec![
        msg(AgentAuthor::User, "q1", 10),
        msg(AgentAuthor::Assistant, "a1", 20),
        msg(AgentAuthor::User, "q2", 30),
        msg(AgentAuthor::Assistant, "a2", 40),
    ];
    let parent = ts.agents.first_mut().unwrap();
    parent.status = AgentStatus::MyTurn;
    parent.archived = true;
    parent.paused = true;
    (ts, gs)
}

#[test]
fn branch_copies_history_up_to_and_including_the_branch_point() {
    let (mut ts, mut gs) = parent_state();
    let id = ts.branch(&mut gs, ("A1", 20), "Alt").unwrap();
    assert_eq!(id, "A2");
    assert_eq!(ts.next_id, 3);

    let history = msgs::of(&ts, &gs, "A2");
    let texts: Vec<_> = history.iter().map(|m| m.content.as_deref().unwrap_or("")).collect();
    assert_eq!(texts, ["q1", "a1"]);
    assert_eq!(history.iter().map(|m| m.timestamp).collect::<Vec<_>>(), [10, 20]);
    assert!(history.iter().all(|m| m.acknowledged));
}

#[test]
fn branch_records_origin_and_gets_its_own_group() {
    let (mut ts, mut gs) = parent_state();
    let _id = ts.branch(&mut gs, ("A1", 20), "Alt").unwrap();
    let branch = ts.agents.iter().find(|t| t.id == "A2").unwrap();
    assert_eq!(branch.origin, Some(AgentOrigin { agent_id: "A1".to_owned(), message_ts: 20 }));
    assert_ne!(branch.human_group_id, ts.agents.first().unwrap().human_group_id);
}

#[test]
fn branch_is_a_fresh_active_agent_and_leaves_the_parent_untouched() {
    let (mut ts, mut gs) = parent_state();
    let id = ts.branch(&mut gs, ("A1", 20), "Alt").unwrap();

    let branch = ts.agents.iter().find(|t| t.id == id).unwrap();
    assert_eq!(branch.name, "Alt");
    assert_eq!(branch.status, AgentStatus::TheirTurn);
    assert!(!branch.archived);
    assert!(!branch.paused);

    let parent = msgs::of(&ts, &gs, "A1");
    assert_eq!(parent.len(), 4);
    assert!(parent.iter().all(|m| !m.acknowledged));
}

#[test]
fn branch_at_last_message_copies_everything() {
    let (mut ts, mut gs) = parent_state();
    let id = ts.branch(&mut gs, ("A1", 40), "Copy").unwrap();
    assert_eq!(msgs::of(&ts, &gs, &id).len(), 4);
}

#[test]
fn branch_rejects_unknown_agent_or_message() {
    let (mut ts, mut gs) = parent_state();
    let unknown_agent = ts.branch(&mut gs, ("A9", 20), "x").unwrap_err();
    assert!(unknown_agent.contains("A9"));
    let unknown_message = ts.branch(&mut gs, ("A1", 25), "x").unwrap_err();
    assert!(unknown_message.contains("ts=25"));
    assert_eq!(ts.agents.len(), 1);
    assert_eq!(ts.next_id, 2);
}

#[test]
fn origin_round_trips_and_defaults_to_none() {
    let legacy: Agent =
        serde_json::from_str(r#"{"id":"A1","name":"n","status":"TheirTurn","messages":[],"created_at":1}"#).unwrap();
    assert_eq!(legacy.origin, None);
    assert!(!serde_json::to_string(&legacy).unwrap().contains("origin"));

    let (mut ts, mut gs) = parent_state();
    let id = ts.branch(&mut gs, ("A1", 30), "b").unwrap();
    let branch = ts.agents.iter().find(|t| t.id == id).unwrap();
    let back: Agent = serde_json::from_str(&serde_json::to_string(branch).unwrap()).unwrap();
    assert_eq!(back.origin, branch.origin);
}

#[test]
fn migration_seeds_human_groups_once() {
    let mut agents = vec![Agent::new("A1".to_owned(), "a".to_owned()), Agent::new("A2".to_owned(), "b".to_owned())];
    let mut gs = GroupsState::new();
    let legacy = |id: &str| vec![msg(AgentAuthor::User, id, 1)];
    assert_eq!(msgs::ensure_human_groups(&mut agents, &mut gs, legacy), 2);
    assert_eq!(msgs::ensure_human_groups(&mut agents, &mut gs, legacy), 0);
    let g = gs.find(&agents.last().unwrap().human_group_id).unwrap();
    assert_eq!((g.owner_agent_id.as_str(), g.messages.len()), ("A2", 1));
}
