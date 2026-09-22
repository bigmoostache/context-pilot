//! Focus-scoping + legacy purge for the thread-owned todo model.
//!
//! The task-editing surface itself now lives in [`crate::yaml`] (the `Todo`
//! tool applies `{prev, new}` diffs to a virtual YAML, reconciled by id). This
//! module keeps only the small pure ops the main crate still calls around
//! that: panel focus-scoping, the one-time legacy backlog purge, and the
//! per-thread purge on thread hard-delete.

use cp_base::state::runtime::State;

use crate::types::TodoState;

/// Drop every item lacking a `thread_id` (the legacy, pre-rework backlog).
/// Called once on load — a permanent, forever purge (FR4).
pub fn purge_threadless(state: &mut State) {
    TodoState::get_mut(state).todos.retain(|t| !t.thread_id.is_empty());
}

/// Remove every item owned by `thread_id` — cascade cleanup when a thread is
/// hard-deleted (FR13; archiving keeps them). Returns the number removed.
pub fn purge_thread_todos(state: &mut State, thread_id: &str) -> usize {
    let ts = TodoState::get_mut(state);
    let before = ts.todos.len();
    ts.todos.retain(|t| t.thread_id != thread_id);
    if ts.nudged_thread.as_deref() == Some(thread_id) {
        ts.nudged_thread = None;
    }
    before.saturating_sub(ts.todos.len())
}

/// Set the injected focused-thread filter used by the panel. Returns whether it
/// changed (which drives the caller's forced panel refresh).
pub fn set_focus_filter(state: &mut State, thread_id: Option<String>) -> bool {
    let ts = TodoState::get_mut(state);
    if ts.focus_filter == thread_id {
        false
    } else {
        ts.focus_filter = thread_id;
        true
    }
}

#[cfg(test)]
mod tests {
    use cp_base::state::runtime::State;

    use super::purge_thread_todos;
    use crate::types::{TodoItem, TodoState, TodoStatus};

    /// A minimal item `id` owned by `thread_id`.
    fn item(id: &str, thread_id: &str, parent_id: Option<&str>) -> TodoItem {
        TodoItem {
            id: id.to_owned(),
            thread_id: thread_id.to_owned(),
            parent_id: parent_id.map(str::to_owned),
            name: id.to_owned(),
            description: String::new(),
            status: TodoStatus::Planned,
            order: 0,
        }
    }

    #[test]
    fn purge_thread_todos_removes_only_that_threads_items() {
        let mut state = State::default();
        let mut ts = TodoState::new();
        ts.todos = vec![item("X1", "T1", None), item("X2", "T1", Some("X1")), item("X3", "T2", None)];
        ts.nudged_thread = Some("T1".to_owned());
        state.set_ext(ts);

        assert_eq!(purge_thread_todos(&mut state, "T1"), 2);

        let after = TodoState::get(&state);
        assert_eq!(after.todos.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["X3"]);
        assert_eq!(after.nudged_thread, None);
    }

    #[test]
    fn purge_thread_todos_is_a_noop_for_an_unknown_thread() {
        let mut state = State::default();
        let mut ts = TodoState::new();
        ts.todos = vec![item("X1", "T1", None)];
        state.set_ext(ts);

        assert_eq!(purge_thread_todos(&mut state, "T9"), 0);
        assert_eq!(TodoState::get(&state).todos.len(), 1);
    }
}
