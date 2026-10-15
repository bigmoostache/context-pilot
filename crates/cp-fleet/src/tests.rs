use super::*;

/// A registry holding `id` in `state`.
fn with(reg: &mut FleetRegistry, id: &str, state: AgentExecState) {
    reg.insert(id.to_owned(), state);
}

#[test]
fn only_streaming_is_active() {
    assert!(AgentExecState::Streaming.is_active());
    assert!(!AgentExecState::Runnable.is_active());
    assert!(!AgentExecState::Idle.is_active());
    assert!(!AgentExecState::Errored.is_active());
}

#[test]
fn facts_priority_streaming_errored_turn() {
    assert_eq!(exec_state_from_facts(true, STUCK_ERROR_THRESHOLD, true), AgentExecState::Streaming);
    assert_eq!(exec_state_from_facts(false, STUCK_ERROR_THRESHOLD, true), AgentExecState::Errored);
    assert_eq!(exec_state_from_facts(false, STUCK_ERROR_THRESHOLD - 1, true), AgentExecState::Runnable);
    assert_eq!(exec_state_from_facts(false, 0, false), AgentExecState::Idle);
}

#[test]
fn registry_basic_ops() {
    let mut reg = FleetRegistry::new();
    assert!(reg.is_empty());
    reg.ensure("A1");
    reg.ensure("A1");
    assert_eq!(reg.len(), 1);
    assert_eq!(reg.get("A1").map(Entry::exec_state), Some(AgentExecState::Idle));
    assert!(!reg.set_state("nope", AgentExecState::Runnable), "unknown id reports false");
    assert!(reg.remove("A1").is_some());
    assert!(reg.is_empty());
}

#[test]
fn stamp_set_on_runnable_edge_and_kept_on_rederive() {
    let mut reg = FleetRegistry::new();
    reg.ensure("A");
    reg.derive_from_turn("A", true);
    let first = reg.get("A").and_then(Entry::waiting_seq);
    assert!(first.is_some());
    reg.derive_from_turn("A", true);
    assert_eq!(reg.get("A").and_then(Entry::waiting_seq), first, "re-deriving Runnable keeps the queue place");
    reg.derive_from_turn("A", false);
    assert_eq!(reg.get("A").and_then(Entry::waiting_seq), None, "leaving Runnable clears the stamp");
}

#[test]
fn derive_leaves_active_and_errored_alone() {
    let mut reg = FleetRegistry::new();
    with(&mut reg, "S", AgentExecState::Streaming);
    with(&mut reg, "E", AgentExecState::Errored);
    reg.derive_from_turn("S", false);
    reg.derive_from_turn("E", true);
    assert_eq!(reg.get("S").map(Entry::exec_state), Some(AgentExecState::Streaming));
    assert_eq!(reg.get("E").map(Entry::exec_state), Some(AgentExecState::Errored));
}

#[test]
fn agent_that_just_ran_requeues_behind_waiters() {
    // Regression: the post-step write used to keep the pre-promotion stamp, so
    // an agent that had just run jumped back ahead of agents still waiting.
    let mut reg = FleetRegistry::new();
    with(&mut reg, "RAN", AgentExecState::Runnable);
    with(&mut reg, "WAIT", AgentExecState::Runnable);
    assert!(reg.set_state("RAN", AgentExecState::Streaming));
    assert!(reg.set_state("RAN", AgentExecState::Runnable));
    assert_eq!(promote(&reg, 1, 0), vec!["WAIT".to_owned()]);
}

#[test]
fn promote_fills_free_slots_oldest_first() {
    let mut reg = FleetRegistry::new();
    for i in 1..K {
        with(&mut reg, &format!("S{i}"), AgentExecState::Streaming);
    }
    with(&mut reg, "OLD", AgentExecState::Runnable);
    with(&mut reg, "NEW", AgentExecState::Runnable);
    assert_eq!(promote(&reg, K, 0), vec!["OLD".to_owned()], "one free slot, oldest wins");
}

#[test]
fn promote_counts_external_active() {
    // Regression: the focused agent's stream was never counted against the cap.
    let mut reg = FleetRegistry::new();
    with(&mut reg, "S", AgentExecState::Streaming);
    with(&mut reg, "W", AgentExecState::Runnable);
    assert_eq!(promote(&reg, 2, 0), vec!["W".to_owned()]);
    assert!(promote(&reg, 2, 1).is_empty(), "focused stream takes the last slot");
}

#[test]
fn promote_none_when_full_or_nobody_waits() {
    let mut reg = FleetRegistry::new();
    with(&mut reg, "S", AgentExecState::Streaming);
    assert!(promote(&reg, 4, 0).is_empty(), "nobody waiting");
    with(&mut reg, "W", AgentExecState::Runnable);
    assert!(promote(&reg, 1, 0).is_empty(), "no free slot");
    with(&mut reg, "E", AgentExecState::Errored);
    assert_eq!(promote(&reg, 4, 0), vec!["W".to_owned()], "errored is never promoted");
}

#[test]
fn promote_bounded_selection_keeps_order() {
    let mut reg = FleetRegistry::new();
    for name in ["A", "B", "C", "D", "E", "F"] {
        with(&mut reg, name, AgentExecState::Runnable);
    }
    assert_eq!(promote(&reg, 3, 0), vec!["A".to_owned(), "B".to_owned(), "C".to_owned()]);
    assert_eq!(promote(&reg, 10, 0).len(), 6);
}

#[test]
fn ids_in_filters_by_state() {
    let mut reg = FleetRegistry::new();
    with(&mut reg, "I", AgentExecState::Idle);
    with(&mut reg, "R", AgentExecState::Runnable);
    assert_eq!(reg.ids_in(AgentExecState::Idle), vec!["I".to_owned()]);
}
