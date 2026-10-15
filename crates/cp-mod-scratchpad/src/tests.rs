//! Regression tests for the round-2 scratchpad defects: per-agent isolation,
//! per-agent ids, legacy load, no-op change stamps, caps.

use cp_base::modules::Module as _;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};
use serde_json::{Value, json};

use crate::ScratchpadModule;
use crate::tools::{execute_create, execute_edit, execute_wipe, purge_agent_cells, purge_agentless};
use crate::types::{MAX_CELL_BYTES, MAX_CELLS_PER_AGENT, ScratchpadCell, ScratchpadState};

/// Fresh state with agent `agent` executing.
fn state_as(agent: &str) -> State {
    let mut state = State::default();
    state.set_ext(ScratchpadState::new());
    state.agent_store.set_executing(Some(agent.to_owned()));
    state
}

/// Run `f` on a tool call named `name` with `input`.
fn call(state: &mut State, f: fn(&ToolUse, &mut State) -> ToolResult, input: Value) -> ToolResult {
    let tool = ToolUse::new("t".to_owned(), "x".to_owned(), input);
    f(&tool, state)
}

/// Create a cell titled `title` for the executing agent.
fn create(state: &mut State, title: &str) -> ToolResult {
    call(state, execute_create, json!({"cell_title": title, "cell_contents": "body"}))
}

/// A cell `id` owned by `agent`.
fn cell(id: &str, agent: &str) -> ScratchpadCell {
    ScratchpadCell { id: id.to_owned(), agent_id: agent.to_owned(), title: id.to_owned(), content: String::new() }
}

/// Current change stamp.
fn rev(state: &State) -> u64 {
    ScratchpadState::get(state).revision
}

#[test]
fn ids_are_numbered_per_agent() {
    let mut state = state_as("A1");
    assert!(create(&mut state, "a").content.contains("C1"));
    assert!(create(&mut state, "b").content.contains("C2"));
    state.agent_store.set_executing(Some("A2".to_owned()));
    assert!(create(&mut state, "c").content.contains("C1"));
    let ss = ScratchpadState::get(&state);
    assert_eq!(ss.agent_cell_count("A1"), 2);
    assert_eq!(ss.agent_cell_count("A2"), 1);
}

#[test]
fn agents_cannot_edit_or_wipe_each_others_cells() {
    let mut state = state_as("A1");
    assert!(!create(&mut state, "mine").is_error);
    state.agent_store.set_executing(Some("A2".to_owned()));
    assert!(!create(&mut state, "theirs").is_error);
    let edit = call(&mut state, execute_edit, json!({"cell_id": "C1", "cell_title": "x"}));
    assert!(!edit.is_error, "A2 edits its own C1");
    assert!(!call(&mut state, execute_wipe, json!({"cell_ids": []})).is_error);
    let ss = ScratchpadState::get(&state);
    assert_eq!(ss.agent_cell_count("A2"), 0);
    assert_eq!(ss.agent_cells("A1").map(|c| c.title.as_str()).collect::<Vec<_>>(), ["mine"]);
}

#[test]
fn rejected_tool_calls_keep_the_change_stamp() {
    let mut state = state_as("A1");
    assert!(!create(&mut state, "a").is_error);
    let before = rev(&state);
    assert!(call(&mut state, execute_edit, json!({"cell_id": "C9", "cell_title": "x"})).is_error);
    assert!(call(&mut state, execute_edit, json!({"cell_id": "C1"})).is_error);
    assert!(!call(&mut state, execute_wipe, json!({"cell_ids": ["C9"]})).is_error);
    assert_eq!(rev(&state), before);
}

#[test]
fn empty_wipes_and_purges_keep_the_change_stamp() {
    let mut state = state_as("A1");
    assert!(!create(&mut state, "a").is_error);
    let before = rev(&state);
    state.agent_store.set_executing(Some("A2".to_owned()));
    assert!(!call(&mut state, execute_wipe, json!({"cell_ids": []})).is_error);
    assert_eq!(purge_agent_cells(&mut state, "A3"), 0);
    purge_agentless(&mut state);
    assert_eq!(rev(&state), before);
}

#[test]
fn caps_reject_oversize_and_overflow() {
    let mut state = state_as("A1");
    let big = "x".repeat(MAX_CELL_BYTES.saturating_add(1));
    assert!(call(&mut state, execute_create, json!({"cell_title": "t", "cell_contents": big})).is_error);
    for i in 0..MAX_CELLS_PER_AGENT {
        assert!(!create(&mut state, &i.to_string()).is_error);
    }
    assert!(create(&mut state, "overflow").is_error);
    let big_edit = "x".repeat(MAX_CELL_BYTES);
    assert!(call(&mut state, execute_edit, json!({"cell_id": "C1", "cell_contents": big_edit})).is_error);
}

#[test]
fn purge_drops_cells_and_counter() {
    let mut state = state_as("A1");
    assert!(!create(&mut state, "a").is_error);
    assert_eq!(purge_agent_cells(&mut state, "A1"), 1);
    assert!(!ScratchpadState::get(&state).next_ids.contains_key("A1"));
}

#[test]
fn legacy_data_loads_and_never_reissues_ids() {
    let mut state = state_as("A1");
    let legacy = json!({
        "scratchpad_cells": [
            {"id": "C7", "thread_id": "A1", "title": "old", "content": ""},
            {"id": "C3", "title": "orphan", "content": ""},
            cell("C12", "A2"),
        ],
        "next_scratchpad_id": 13u64,
    });
    ScratchpadModule::new().load_module_data(&legacy, &mut state);
    let ss = ScratchpadState::get(&state);
    assert_eq!(ss.scratchpad_cells.len(), 2, "agentless cell purged");
    assert!(ss.has_cell("A1", "C7"));
    assert!(create(&mut state, "new").content.contains("C8"));

    let saved = ScratchpadModule::new().save_module_data(&state);
    let mut reloaded = state_as("A2");
    ScratchpadModule::new().load_module_data(&saved, &mut reloaded);
    assert!(create(&mut reloaded, "n").content.contains("C13"));
}
