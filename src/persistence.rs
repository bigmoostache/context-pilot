use std::fs;
use std::path::PathBuf;
use std::process;

use crate::constants::{STORE_DIR, STATE_FILE, MESSAGES_DIR};
use crate::state::{Message, PersistedState, State};
use crate::tool_defs::{get_all_tool_definitions, ToolDefinition};
use crate::tools::MANAGE_TOOLS_ID;

/// Get current process PID
fn current_pid() -> u32 {
    process::id()
}

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

/// Build tools from defaults, applying disabled_tools list
fn build_tools_from_disabled(disabled_tools: &[String]) -> Vec<ToolDefinition> {
    let mut tools = get_all_tool_definitions();
    for tool in &mut tools {
        // manage_tools can never be disabled
        if tool.id != MANAGE_TOOLS_ID && disabled_tools.contains(&tool.id) {
            tool.enabled = false;
        }
    }
    tools
}

/// Extract disabled tool IDs from tools list
fn extract_disabled_tools(tools: &[ToolDefinition]) -> Vec<String> {
    tools.iter()
        .filter(|t| !t.enabled)
        .map(|t| t.id.clone())
        .collect()
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
                input: persisted.draft_input,
                input_cursor: persisted.draft_cursor,
                selected_context: persisted.selected_context,
                is_streaming: false,
                scroll_offset: 0.0,
                user_scrolled: false,
                scroll_accel: 1.0,
                max_scroll: 0.0,
                streaming_estimated_tokens: 0,
                tree_filter: persisted.tree_filter,
                tree_open_folders: open_folders,
                tree_descriptions: persisted.tree_descriptions,
                pending_tldrs: 0,
                next_user_id: persisted.next_user_id,
                next_assistant_id: persisted.next_assistant_id,
                next_tool_id: persisted.next_tool_id,
                next_result_id: persisted.next_result_id,
                todos: persisted.todos,
                next_todo_id: persisted.next_todo_id,
                memories: persisted.memories,
                next_memory_id: persisted.next_memory_id,
                tools: build_tools_from_disabled(&persisted.disabled_tools),
                is_cleaning_context: false,
                dirty: true,
                spinner_frame: 0,
                dev_mode: persisted.dev_mode,
                // Git status defaults (runtime-only, fetched on startup)
                git_branch: None,
                git_is_repo: false,
                git_file_changes: vec![],
                git_last_refresh_ms: 0,
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
        draft_input: state.input.clone(),
        draft_cursor: state.input_cursor,
        tree_filter: state.tree_filter.clone(),
        tree_open_folders: state.tree_open_folders.clone(),
        tree_descriptions: state.tree_descriptions.clone(),
        next_user_id: state.next_user_id,
        next_assistant_id: state.next_assistant_id,
        next_tool_id: state.next_tool_id,
        next_result_id: state.next_result_id,
        todos: state.todos.clone(),
        next_todo_id: state.next_todo_id,
        memories: state.memories.clone(),
        next_memory_id: state.next_memory_id,
        disabled_tools: extract_disabled_tools(&state.tools),
        owner_pid: Some(current_pid()),
        dev_mode: state.dev_mode,
    };

    let path = dir.join(STATE_FILE);
    if let Ok(json) = serde_json::to_string_pretty(&persisted) {
        fs::write(path, json).ok();
    }
}

/// Check if we still own the state file (another instance may have taken over)
/// Returns false if another process has claimed ownership
pub fn check_ownership() -> bool {
    let path = PathBuf::from(STORE_DIR).join(STATE_FILE);

    if let Ok(json) = fs::read_to_string(&path) {
        if let Ok(persisted) = serde_json::from_str::<PersistedState>(&json) {
            if let Some(owner) = persisted.owner_pid {
                return owner == current_pid();
            }
        }
    }

    // If we can't read the file or there's no owner, assume we're still the owner
    true
}
