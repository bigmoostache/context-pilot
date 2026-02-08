/// Persistence module for multi-worker state management
///
/// This module handles the file-based persistence of:
/// - SharedConfig (config.json) - Global settings shared across workers
/// - WorkerState (states/{worker}.json) - Worker-specific state
/// - PanelData (panels/{uid}.json) - Dynamic panel metadata
/// - Messages (messages/{uid}.yaml) - Conversation messages

pub mod config;
pub mod worker;
pub mod panel;
pub mod message;

// Re-export commonly used functions
pub use message::{load_message, save_message, delete_message};
pub use config::current_pid;

use std::fs;
use std::path::PathBuf;
use std::collections::HashMap;
use chrono::Local;

use crate::config::set_active_theme;
use crate::constants::{STORE_DIR, CONFIG_FILE, DEFAULT_WORKER_ID};
use crate::state::{SharedConfig, WorkerState, PanelData, Message, State, ContextType, ContextElement};

/// Errors directory name
const ERRORS_DIR: &str = "errors";

/// Check if new multi-file format exists
fn new_format_exists() -> bool {
    PathBuf::from(STORE_DIR).join(CONFIG_FILE).exists()
}

/// Load state using new multi-file format
pub fn load_state() -> State {
    if new_format_exists() {
        load_state_new()
    } else {
        // Fresh start - create default state
        let state = State::default();
        set_active_theme(&state.active_theme);
        state
    }
}

/// Load state from the new multi-file format
/// Convert PanelData to ContextElement
fn panel_to_context(panel: &PanelData, local_id: &str) -> ContextElement {
    ContextElement {
        id: local_id.to_string(),
        uid: Some(panel.uid.clone()),
        context_type: panel.panel_type,
        name: panel.name.clone(),
        token_count: panel.token_count,
        file_path: panel.file_path.clone(),
        file_hash: None,
        glob_pattern: panel.glob_pattern.clone(),
        glob_path: panel.glob_path.clone(),
        grep_pattern: panel.grep_pattern.clone(),
        grep_path: panel.grep_path.clone(),
        grep_file_pattern: panel.grep_file_pattern.clone(),
        tmux_pane_id: panel.tmux_pane_id.clone(),
        tmux_lines: panel.tmux_lines,
        tmux_last_keys: None,
        tmux_description: panel.tmux_description.clone(),
        cached_content: None,
        cache_deprecated: true,  // Will be refreshed on load
        // Use saved timestamp if available, otherwise current time for new panels
        last_refresh_ms: if panel.last_refresh_ms > 0 { panel.last_refresh_ms } else { crate::core::panels::now_ms() },
        tmux_last_lines_hash: None,
        current_page: 0,
        total_pages: 1,
        full_token_count: 0,
    }
}

fn load_state_new() -> State {
    // Load shared config
    let shared_config = config::load_config().unwrap_or_default();

    // Load worker state (main_worker)
    let worker_state = worker::load_worker(DEFAULT_WORKER_ID).unwrap_or_default();

    // Build context from panels in panels/ folder
    let mut context: Vec<ContextElement> = Vec::new();
    let important = &worker_state.important_panel_uids;

    // Load fixed panels in canonical order (P0-P7) from module registry
    let defaults = crate::modules::all_fixed_panel_defaults();
    for (pos, (_, _, ct, name, cache_deprecated)) in defaults.iter().enumerate() {
        let id = format!("P{}", pos);
        if *ct == ContextType::System {
            // System panel is not stored in panels/ - comes from systems[]
            context.push(crate::modules::make_default_context_element(&id, *ct, name, *cache_deprecated));
        } else if let Some(uid) = important.get(ct) {
            if let Some(panel_data) = panel::load_panel(uid) {
                context.push(panel_to_context(&panel_data, &id));
            }
        }
    }

    // Load dynamic panels from panel_uid_to_local_id (P8+)
    let mut dynamic_panels: Vec<(String, ContextElement)> = worker_state.panel_uid_to_local_id.iter()
        .filter_map(|(uid, local_id)| {
            panel::load_panel(uid).map(|p| (local_id.clone(), panel_to_context(&p, local_id)))
        })
        .collect();
    // Sort by local ID to maintain order
    dynamic_panels.sort_by(|a, b| {
        let a_num: usize = a.0.trim_start_matches('P').parse().unwrap_or(999);
        let b_num: usize = b.0.trim_start_matches('P').parse().unwrap_or(999);
        a_num.cmp(&b_num)
    });
    for (_, elem) in dynamic_panels {
        context.push(elem);
    }

    // Load messages from the conversation panel
    let message_uids: Vec<String> = important.get(&ContextType::Conversation)
        .and_then(|uid| panel::load_panel(uid))
        .map(|p| p.message_uids)
        .unwrap_or_default();

    let messages: Vec<Message> = message_uids.iter()
        .filter_map(|uid| load_message(uid))
        .collect();

    // Calculate display ID counters from loaded messages
    let next_user_id = messages.iter()
        .filter(|m| m.id.starts_with('U'))
        .filter_map(|m| m.id[1..].parse::<usize>().ok())
        .max()
        .map(|n| n + 1)
        .unwrap_or(1);
    let next_assistant_id = messages.iter()
        .filter(|m| m.id.starts_with('A'))
        .filter_map(|m| m.id[1..].parse::<usize>().ok())
        .max()
        .map(|n| n + 1)
        .unwrap_or(1);

    // Start with default state, then apply infrastructure + module data
    let mut state = State {
        context,
        messages,
        selected_context: shared_config.selected_context,
        next_user_id,
        next_assistant_id,
        next_tool_id: worker_state.next_tool_id,
        next_result_id: worker_state.next_result_id,
        input: shared_config.draft_input,
        input_cursor: shared_config.draft_cursor,
        active_theme: shared_config.active_theme.clone(),
        ..State::default()
    };

    // Load module data from appropriate config (global → SharedConfig, worker → WorkerState)
    let null = serde_json::Value::Null;
    for module in crate::modules::all_modules() {
        let data = if module.is_global() {
            shared_config.modules.get(module.id()).unwrap_or(&null)
        } else {
            worker_state.modules.get(module.id()).unwrap_or(&null)
        };
        module.load_module_data(data, &mut state);
    }

    // If tools weren't built by core module's load_module_data (e.g., no saved data),
    // ensure tools are built from active_modules
    if state.tools.is_empty() {
        state.tools = crate::modules::active_tool_definitions(&state.active_modules);
    }

    // Set the global active theme
    set_active_theme(&state.active_theme);
    state
}

/// Save state using new multi-file format
pub fn save_state(state: &State) {
    let dir = PathBuf::from(STORE_DIR);
    fs::create_dir_all(&dir).ok();

    // Build module data maps
    let mut global_modules = HashMap::new();
    let mut worker_modules = HashMap::new();
    for module in crate::modules::all_modules() {
        let data = module.save_module_data(state);
        if !data.is_null() {
            if module.is_global() {
                global_modules.insert(module.id().to_string(), data);
            } else {
                worker_modules.insert(module.id().to_string(), data);
            }
        }
    }

    // Create SharedConfig (infrastructure + global module data)
    let shared_config = SharedConfig {
        reload_requested: false,
        active_theme: state.active_theme.clone(),
        owner_pid: Some(current_pid()),
        selected_context: state.selected_context,
        draft_input: state.input.clone(),
        draft_cursor: state.input_cursor,
        modules: global_modules,
    };

    // Build important_panel_uids from fixed context elements (all except System)
    let mut important_uids: HashMap<ContextType, String> = HashMap::new();
    for ctx in &state.context {
        if ctx.context_type.is_fixed() && ctx.context_type != ContextType::System {
            if let Some(uid) = &ctx.uid {
                important_uids.insert(ctx.context_type, uid.clone());
            }
        }
    }

    // Build panel_uid_to_local_id for dynamic panels (P8+)
    let panel_uid_to_local_id: HashMap<String, String> = state.context.iter()
        .filter(|c| c.uid.is_some() && !c.context_type.is_fixed())
        .filter_map(|c| c.uid.as_ref().map(|uid| (uid.clone(), c.id.clone())))
        .collect();

    // Create WorkerState (infrastructure + worker module data)
    let worker_state = WorkerState {
        worker_id: DEFAULT_WORKER_ID.to_string(),
        important_panel_uids: important_uids.clone(),
        panel_uid_to_local_id,
        next_tool_id: state.next_tool_id,
        next_result_id: state.next_result_id,
        modules: worker_modules,
    };

    // Save shared config
    config::save_config(&shared_config);

    // Save worker state
    worker::save_worker(&worker_state);

    // Save ALL panels to panels/ folder (except System P0 which comes from systems[])
    for ctx in state.context.iter() {
        // Skip System panel (P0) - it comes from systems[] in SharedConfig
        if ctx.context_type == ContextType::System {
            continue;
        }

        // All other panels need a UID to be saved
        if let Some(uid) = &ctx.uid {
            let panel_data = PanelData {
                uid: uid.clone(),
                panel_type: ctx.context_type,
                name: ctx.name.clone(),
                token_count: ctx.token_count,
                last_refresh_ms: ctx.last_refresh_ms,
                // Conversation panel gets message_uids
                message_uids: if ctx.context_type == ContextType::Conversation {
                    state.messages.iter()
                        .map(|m| m.uid.clone().unwrap_or_else(|| m.id.clone()))
                        .collect()
                } else {
                    vec![]
                },
                file_path: ctx.file_path.clone(),
                glob_pattern: ctx.glob_pattern.clone(),
                glob_path: ctx.glob_path.clone(),
                grep_pattern: ctx.grep_pattern.clone(),
                grep_path: ctx.grep_path.clone(),
                grep_file_pattern: ctx.grep_file_pattern.clone(),
                tmux_pane_id: ctx.tmux_pane_id.clone(),
                tmux_lines: ctx.tmux_lines,
                tmux_description: ctx.tmux_description.clone(),
            };
            panel::save_panel(&panel_data);
        }
    }
}

/// Check if we still own the state file (another instance may have taken over)
/// Returns false if another process has claimed ownership
pub fn check_ownership() -> bool {
    if let Some(cfg) = config::load_config() {
        if let Some(owner) = cfg.owner_pid {
            return owner == current_pid();
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
