//! Focus-scoping + legacy purge for the thread-owned todo model.
//!
//! Task editing lives in [`crate::upsert`] (the `Todo` tool's structured
//! upsert). This module keeps the small ops the main crate calls around it:
//! panel focus-scoping, the one-time legacy backlog purge, and the per-agent
//! purge on agent hard-delete.

use cp_base::state::runtime::State;

use crate::types::TodoState;

/// Drop every item lacking a `agent_id` (the legacy, pre-rework backlog).
/// Called once on load — a permanent, forever purge (FR4).
pub fn purge_agentless(state: &mut State) {
    TodoState::get_mut(state).todos.retain(|t| !t.agent_id.is_empty());
}

/// Remove every item owned by `agent_id` — cascade cleanup when an agent is
/// hard-deleted (FR13; archiving keeps them). Returns the number removed.
pub fn purge_agent_todos(state: &mut State, agent_id: &str) -> usize {
    let ts = TodoState::get_mut(state);
    let before = ts.todos.len();
    ts.todos.retain(|t| t.agent_id != agent_id);
    if ts.nudged_agent.as_deref() == Some(agent_id) {
        ts.nudged_agent = None;
    }
    before.saturating_sub(ts.todos.len())
}

/// Set the injected focused-thread filter used by the panel. Returns whether it
/// changed (which drives the caller's forced panel refresh).
pub fn set_focus_filter(state: &mut State, agent_id: Option<String>) -> bool {
    // Read first: `get_mut` refreshes the change stamp, and this runs every tick.
    if TodoState::get(state).focus_filter == agent_id {
        return false;
    }
    TodoState::get_mut(state).focus_filter = agent_id;
    true
}

#[cfg(test)]
mod tests {
    use cp_base::state::runtime::State;

    use super::purge_agent_todos;
    use crate::types::{TodoItem, TodoState, TodoStatus};

    /// A minimal item `id` owned by `agent_id`.
    fn item(id: &str, agent_id: &str, parent_id: Option<&str>) -> TodoItem {
        TodoItem {
            id: id.to_owned(),
            agent_id: agent_id.to_owned(),
            parent_id: parent_id.map(str::to_owned),
            name: id.to_owned(),
            description: String::new(),
            status: TodoStatus::Planned,
            order: 0,
        }
    }

    #[test]
    fn purge_agent_todos_removes_only_that_agents_items() {
        let mut state = State::default();
        let mut ts = TodoState::new();
        ts.todos = vec![item("X1", "T1", None), item("X2", "T1", Some("X1")), item("X3", "T2", None)];
        ts.nudged_agent = Some("T1".to_owned());
        state.set_ext(ts);

        assert_eq!(purge_agent_todos(&mut state, "T1"), 2);

        let after = TodoState::get(&state);
        assert_eq!(after.todos.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["X3"]);
        assert_eq!(after.nudged_agent, None);
    }

    #[test]
    fn purge_agent_todos_is_a_noop_for_an_unknown_agent() {
        let mut state = State::default();
        let mut ts = TodoState::new();
        ts.todos = vec![item("X1", "T1", None)];
        state.set_ext(ts);

        assert_eq!(purge_agent_todos(&mut state, "T9"), 0);
        assert_eq!(TodoState::get(&state).todos.len(), 1);
    }
}
