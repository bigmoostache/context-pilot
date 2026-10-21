//! Focus-scoping + legacy purge for the thread-owned todo model.
//!
//! Task editing lives in [`crate::upsert`] (the `Todo` tool's structured
//! upsert). This module keeps the small ops the main crate calls around it:
//! panel focus-scoping, the one-time legacy backlog purge, and the per-agent
//! purge on agent hard-delete.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cp_base::state::runtime::State;

use crate::types::{TodoItem, TodoState, TodoStatus};

/// Hard-remove every task in `roots` together with its whole subtree.
/// Returns the number of tasks removed.
pub fn delete_subtrees(state: &mut State, roots: &BTreeSet<String>) -> usize {
    let mut doomed = roots.clone();
    let todos = &TodoState::get(state).todos;
    // Fixpoint: storage order is not topological, so sweep until no new
    // descendant joins.
    loop {
        let before = doomed.len();
        for t in todos {
            if t.parent_id.as_ref().is_some_and(|p| doomed.contains(p)) {
                let _new = doomed.insert(t.id.clone());
            }
        }
        if doomed.len() == before {
            break;
        }
    }
    remove_ids(state, &doomed)
}

/// Auto-clean `agent_id`'s tree: when every child of a task is finished
/// (done or cancelled, recursively), drop those children. The parent stays
/// with its own status. Returns the number of tasks removed.
pub fn prune_finished(state: &mut State, agent_id: &str) -> usize {
    let doomed = finished_children(TodoState::get(state), agent_id);
    delete_subtrees(state, &doomed)
}

/// Ids of every task whose sibling group is entirely finished-subtree.
fn finished_children(ts: &TodoState, agent_id: &str) -> BTreeSet<String> {
    let mut kids: BTreeMap<&str, Vec<&TodoItem>> = BTreeMap::new();
    for t in ts.todos.iter().filter(|t| t.agent_id == agent_id) {
        if let Some(parent) = t.parent_id.as_deref() {
            kids.entry(parent).or_default().push(t);
        }
    }
    let mut memo: HashMap<&str, bool> = HashMap::new();
    let mut doomed = BTreeSet::new();
    for children in kids.values() {
        if children.iter().all(|c| subtree_finished(c, &kids, &mut memo)) {
            doomed.extend(children.iter().map(|c| c.id.clone()));
        }
    }
    doomed
}

/// Whether `task` and all its descendants are done or cancelled (memoized).
fn subtree_finished<'tree>(
    task: &'tree TodoItem,
    kids: &BTreeMap<&'tree str, Vec<&'tree TodoItem>>,
    memo: &mut HashMap<&'tree str, bool>,
) -> bool {
    if let Some(&known) = memo.get(task.id.as_str()) {
        return known;
    }
    let finished = matches!(task.status, TodoStatus::Done | TodoStatus::Cancelled)
        && kids.get(task.id.as_str()).is_none_or(|cs| cs.iter().all(|c| subtree_finished(c, kids, memo)));
    let _prev = memo.insert(task.id.as_str(), finished);
    finished
}

/// Remove every task whose id is in `doomed`; returns how many went.
fn remove_ids(state: &mut State, doomed: &BTreeSet<String>) -> usize {
    if doomed.is_empty() {
        return 0;
    }
    let ts = TodoState::get_mut(state);
    let before = ts.todos.len();
    ts.todos.retain(|t| !doomed.contains(&t.id));
    before.saturating_sub(ts.todos.len())
}

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

    /// Run one upsert payload on a fresh-or-existing state for agent `A1`.
    fn run(state: &mut State, payload: &serde_json::Value) -> Result<Vec<String>, String> {
        let items = crate::upsert::parse_items(payload)?;
        crate::upsert::upsert(state, "A1", &items)
    }

    /// Ids still stored, in storage order.
    fn ids(state: &State) -> Vec<String> {
        TodoState::get(state).todos.iter().map(|t| t.id.clone()).collect()
    }

    /// State with tree X1{X2, X3{X4}} for agent `A1`.
    fn seeded() -> State {
        let mut state = State::default();
        state.set_ext(TodoState::new());
        let tree = serde_json::json!([{"title": "r", "children": [{"title": "a"}, {"title": "b", "children": [{"title": "c"}]}]}]);
        let _created = run(&mut state, &tree).unwrap();
        state
    }

    #[test]
    fn deleted_removes_whole_subtree() {
        let mut state = seeded();
        let _ok = run(&mut state, &serde_json::json!([{"id": "X3", "status": "deleted"}])).unwrap();
        assert_eq!(ids(&state), ["X1", "X2"]);
    }

    #[test]
    fn deleted_on_create_is_rejected() {
        let mut state = seeded();
        let _err = run(&mut state, &serde_json::json!([{"title": "n", "status": "deleted"}])).unwrap_err();
        assert_eq!(ids(&state).len(), 4);
    }

    #[test]
    fn finished_children_are_pruned_only_when_all_finished() {
        let mut state = seeded();
        let flip = serde_json::json!([{"id": "X2", "status": "done"}, {"id": "X4", "status": "cancelled"}]);
        let _ok = run(&mut state, &flip).unwrap();
        // X4 pruned (X3's only child finished); X2 kept (X3 still planned).
        assert_eq!(ids(&state), ["X1", "X2", "X3"]);
        let _closed = run(&mut state, &serde_json::json!([{"id": "X3", "status": "done"}])).unwrap();
        assert_eq!(ids(&state), ["X1"]);
        assert_eq!(TodoState::get(&state).todos.first().map(|t| t.status), Some(TodoStatus::Planned));
    }
}
