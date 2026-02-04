use std::fs;
use std::path::PathBuf;
use std::process;
use chrono::Local;

use crate::constants::{STORE_DIR, STATE_FILE, MESSAGES_DIR};

/// Errors directory name
const ERRORS_DIR: &str = "errors";
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

/// Migrate old context element names to new names
fn migrate_context_names(context: &mut [crate::state::ContextElement]) {
    for elem in context.iter_mut() {
        // Migrate old names to new names (feature-dump rename)
        match (elem.id.as_str(), elem.name.as_str()) {
            ("P1", "Main") => elem.name = "Chat".to_string(),
            ("P3", "Todo") => elem.name = "WIP".to_string(),
            ("P4", "Memory") => elem.name = "Notes".to_string(),
            ("P5", "Overview") => elem.name = "World".to_string(),
            ("P6", "Git") => elem.name = "Changes".to_string(),
            _ => {}
        }
    }
}

pub fn load_state() -> State {
    let path = PathBuf::from(STORE_DIR).join(STATE_FILE);

    if let Ok(json) = fs::read_to_string(&path) {
        if let Ok(mut persisted) = serde_json::from_str::<PersistedState>(&json) {
            // Migrate old context names to new names
            migrate_context_names(&mut persisted.context);
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
                systems: persisted.systems,
                next_system_id: persisted.next_system_id,
                active_system_id: persisted.active_system_id,
                scratchpad_cells: persisted.scratchpad_cells,
                next_scratchpad_id: persisted.next_scratchpad_id,
                tools: build_tools_from_disabled(&persisted.disabled_tools),
                is_cleaning_context: false,
                dirty: true,
                spinner_frame: 0,
                dev_mode: persisted.dev_mode,
                perf_enabled: false, // Runtime only, not persisted
                config_view: false, // Runtime only
                config_selected_bar: 0,
                llm_provider: persisted.llm_provider,
                anthropic_model: persisted.anthropic_model,
                grok_model: persisted.grok_model,
                groq_model: persisted.groq_model,
                cleaning_threshold: persisted.cleaning_threshold,
                cleaning_target_proportion: persisted.cleaning_target_proportion,
                context_budget: persisted.context_budget,
                // API check defaults (runtime-only)
                api_check_in_progress: false,
                api_check_result: None,
                // Git status defaults (runtime-only, fetched on startup)
                git_branch: None,
                git_branches: vec![],
                git_is_repo: false,
                git_file_changes: vec![],
                git_last_refresh_ms: 0,
                git_show_diffs: persisted.git_show_diffs,
                git_status_hash: None,
                git_show_logs: false,
                git_log_args: None,
                git_log_content: None,
                // API retry (runtime-only)
                api_retry_count: 0,
                reload_pending: false,
                waiting_for_panels: false,
                // Render cache (runtime-only)
                last_viewport_width: 0,
                message_cache: std::collections::HashMap::new(),
                input_cache: None,
                full_content_cache: None,
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
        systems: state.systems.clone(),
        next_system_id: state.next_system_id,
        active_system_id: state.active_system_id.clone(),
        scratchpad_cells: state.scratchpad_cells.clone(),
        next_scratchpad_id: state.next_scratchpad_id,
        disabled_tools: extract_disabled_tools(&state.tools),
        owner_pid: Some(current_pid()),
        dev_mode: state.dev_mode,
        git_show_diffs: state.git_show_diffs,
        llm_provider: state.llm_provider,
        anthropic_model: state.anthropic_model,
        grok_model: state.grok_model,
        groq_model: state.groq_model,
        cleaning_threshold: state.cleaning_threshold,
        cleaning_target_proportion: state.cleaning_target_proportion,
        context_budget: state.context_budget,
        reload_requested: false, // Always clear on save
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

/// Log an error to .context-pilot/errors/ and return the file path
pub fn log_error(error: &str) -> String {
    let errors_dir = PathBuf::from(STORE_DIR).join(ERRORS_DIR);
    fs::create_dir_all(&errors_dir).ok();

    // Count existing error files to determine next number
    let error_count = fs::read_dir(&errors_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map(|ext| ext == "txt").unwrap_or(false))
                .count()
        })
        .unwrap_or(0);

    let error_num = error_count + 1;
    let filename = format!("error_{}.txt", error_num);
    let filepath = errors_dir.join(&filename);

    // Create error log content with timestamp
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    let content = format!(
        "Error Log #{}\n\
         Timestamp: {}\n\
         \n\
         Error Details:\n\
         {}\n",
        error_num, timestamp, error
    );

    fs::write(&filepath, content).ok();

    filepath.to_string_lossy().to_string()
}
