use std::fs;
use std::path::PathBuf;

use crate::constants::{STORE_DIR, STATE_FILE, MESSAGES_DIR};
use crate::state::{Message, PersistedState, State};
use crate::tool_defs::{get_all_tool_definitions, ToolDefinition};

fn messages_dir() -> PathBuf {
    PathBuf::from(STORE_DIR).join(MESSAGES_DIR)
}

fn message_path(id: &str) -> PathBuf {
    messages_dir().join(format!("{}.yaml", id))
}

pub fn load_message(id: &str) -> Option<Message> {
    let path = message_path(id);
    let yaml = fs::read_to_string(&path).ok()?;
    serde_yaml::from_str(&yaml).ok()
}

pub fn save_message(msg: &Message) {
    let dir = messages_dir();
    fs::create_dir_all(&dir).ok();
    let path = message_path(&msg.id);
    if let Ok(yaml) = serde_yaml::to_string(msg) {
        fs::write(path, yaml).ok();
    }
}

pub fn delete_message(id: &str) {
    let path = message_path(id);
    fs::remove_file(path).ok();
}

/// Merge persisted tools with current tool definitions.
/// Preserves enabled state for existing tools, adds new tools as enabled.
fn merge_tools(persisted: Vec<ToolDefinition>) -> Vec<ToolDefinition> {
    let current = get_all_tool_definitions();
    current.into_iter().map(|mut tool| {
        // If tool existed in persisted state, preserve its enabled state
        if let Some(old) = persisted.iter().find(|t| t.id == tool.id) {
            tool.enabled = old.enabled;
        }
        tool
    }).collect()
}

pub fn load_state() -> State {
    let path = PathBuf::from(STORE_DIR).join(STATE_FILE);

    if let Ok(json) = fs::read_to_string(&path) {
        if let Ok(persisted) = serde_json::from_str::<PersistedState>(&json) {
            let messages: Vec<Message> = persisted.message_ids
                .iter()
                .filter_map(|id| load_message(id))
                .collect();

            // Ensure root is always open
            let mut open_folders = persisted.tree_open_folders;
            if !open_folders.contains(&".".to_string()) {
                open_folders.insert(0, ".".to_string());
            }

            return State {
                context: persisted.context,
                messages,
                input: String::new(),
                input_cursor: 0,
                selected_context: persisted.selected_context,
                is_streaming: false,
                scroll_offset: 0.0,
                user_scrolled: false,
                scroll_accel: 1.0,
                max_scroll: 0.0,
                streaming_estimated_tokens: 0,
                copy_mode: false,
                tree_filter: persisted.tree_filter,
                tree_open_folders: open_folders,
                tree_descriptions: persisted.tree_descriptions,
                pending_tldrs: 0,
                next_user_id: persisted.next_user_id,
                next_assistant_id: persisted.next_assistant_id,
                next_tool_id: persisted.next_tool_id,
                next_result_id: persisted.next_result_id,
                next_context_id: persisted.next_context_id,
                todos: persisted.todos,
                next_todo_id: persisted.next_todo_id,
                memories: persisted.memories,
                next_memory_id: persisted.next_memory_id,
                tools: merge_tools(persisted.tools),
                is_cleaning_context: false,
            };
        }
    }

    State::default()
}

pub fn save_state(state: &State) {
    let dir = PathBuf::from(STORE_DIR);
    fs::create_dir_all(&dir).ok();

    let persisted = PersistedState {
        context: state.context.clone(),
        message_ids: state.messages.iter().map(|m| m.id.clone()).collect(),
        selected_context: state.selected_context,
        tree_filter: state.tree_filter.clone(),
        tree_open_folders: state.tree_open_folders.clone(),
        tree_descriptions: state.tree_descriptions.clone(),
        next_user_id: state.next_user_id,
        next_assistant_id: state.next_assistant_id,
        next_tool_id: state.next_tool_id,
        next_result_id: state.next_result_id,
        next_context_id: state.next_context_id,
        todos: state.todos.clone(),
        next_todo_id: state.next_todo_id,
        memories: state.memories.clone(),
        next_memory_id: state.next_memory_id,
        tools: state.tools.clone(),
    };

    let path = dir.join(STATE_FILE);
    if let Ok(json) = serde_json::to_string_pretty(&persisted) {
        fs::write(path, json).ok();
    }
}
