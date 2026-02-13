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
pub mod writer;

// Re-export commonly used functions
pub use message::{load_message, save_message, delete_message};
pub use config::current_pid;
pub use writer::{PersistenceWriter, WriteBatch, WriteOp, DeleteOp};

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
        result_command: panel.result_command.clone(),
        result_command_hash: panel.result_command_hash.clone(),
        skill_prompt_id: panel.skill_prompt_id.clone(),
        cached_content: None,
        history_messages: None,
        cache_deprecated: true,  // Will be refreshed on load
        cache_in_flight: false,
        // Use saved timestamp if available, otherwise current time for new panels
        last_refresh_ms: if panel.last_refresh_ms > 0 { panel.last_refresh_ms } else { crate::core::panels::now_ms() },
        last_polled_ms: 0,
        content_hash: panel.content_hash.clone(),
        tmux_last_lines_hash: None,
        current_page: 0,
        total_pages: 1,
        full_token_count: 0,
        panel_cache_hit: false,
        panel_total_cost: panel.panel_total_cost.unwrap_or(0.0),
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

    // Load Conversation panel (special: not in FIXED_PANEL_ORDER, uses id "chat")
    if let Some(uid) = important.get(&ContextType::Conversation) {
        if let Some(panel_data) = panel::load_panel(uid) {
            context.push(panel_to_context(&panel_data, "chat"));
        }
    }

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
            panel::load_panel(uid).map(|p| {
                let mut elem = panel_to_context(&p, local_id);

                // For ConversationHistory panels, load history messages and rebuild cached content
                if p.panel_type == ContextType::ConversationHistory && !p.message_uids.is_empty() {
                    let msgs: Vec<Message> = p.message_uids.iter()
                        .filter_map(|uid| load_message(uid))
                        .collect();
                    if !msgs.is_empty() {
                        // Rebuild cached_content using format_chunk_content logic
                        let mut content = String::new();
                        for msg in &msgs {
                            use crate::state::{MessageStatus, MessageType};
                            if msg.status == MessageStatus::Deleted || msg.status == MessageStatus::Detached {
                                continue;
                            }
                            match msg.message_type {
                                MessageType::ToolCall => {
                                    for tu in &msg.tool_uses {
                                        content += &format!(
                                            "tool_call {}({})\n",
                                            tu.name,
                                            serde_json::to_string(&tu.input).unwrap_or_default()
                                        );
                                    }
                                }
                                MessageType::ToolResult => {
                                    for tr in &msg.tool_results {
                                        content += &format!("{}\n", tr.content);
                                    }
                                }
                                MessageType::TextMessage => {
                                    let text = match msg.status {
                                        MessageStatus::Summarized => {
                                            msg.tl_dr.as_deref().unwrap_or(&msg.content)
                                        }
                                        _ => &msg.content,
                                    };
                                    if !text.is_empty() {
                                        content += &format!("[{}]: {}\n", msg.role, text);
                                    }
                                }
                            }
                        }
                        let token_count = crate::state::estimate_tokens(&content);
                        let total_pages = crate::state::compute_total_pages(token_count);
                        elem.cached_content = Some(content);
                        elem.history_messages = Some(msgs);
                        elem.token_count = token_count;
                        elem.total_pages = total_pages;
                        elem.full_token_count = token_count;
                        elem.cache_deprecated = false;
                    }
                }

                (local_id.clone(), elem)
            })
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

        // Always load worker-specific data from worker state
        let worker_data = worker_state.modules.get(&format!("{}_worker", module.id())).unwrap_or(&null);
        module.load_worker_data(worker_data, &mut state);
    }

    // If tools weren't built by core module's load_module_data (e.g., no saved data),
    // ensure tools are built from active_modules
    if state.tools.is_empty() {
        state.tools = crate::modules::active_tool_definitions(&state.active_modules);
    }

    // Load GitHub token from environment
    dotenvy::dotenv().ok();
    state.github_token = std::env::var("GITHUB_TOKEN").ok();

    // Set the global active theme
    set_active_theme(&state.active_theme);
    state
}

/// Build a WriteBatch from the current state (CPU work only — no I/O).
/// This serializes all config, worker state, panels, and history messages
/// into a batch of file write/delete operations.
pub fn build_save_batch(state: &State) -> WriteBatch {
    let _guard = crate::profile!("persist::build_save_batch");
    let dir = PathBuf::from(STORE_DIR);
    let mut writes = Vec::new();
    let mut deletes = Vec::new();
    let ensure_dirs = vec![
        dir.clone(),
        dir.join(crate::constants::STATES_DIR),
        dir.join(crate::constants::PANELS_DIR),
        dir.join(crate::constants::MESSAGES_DIR),
        dir.join(crate::constants::LOGS_DIR),
    ];

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
        let worker_data = module.save_worker_data(state);
        if !worker_data.is_null() {
            worker_modules.insert(format!("{}_worker", module.id()), worker_data);
        }
    }

    // SharedConfig
    let shared_config = SharedConfig {
        reload_requested: false,
        active_theme: state.active_theme.clone(),
        owner_pid: Some(current_pid()),
        selected_context: state.selected_context,
        draft_input: state.input.clone(),
        draft_cursor: state.input_cursor,
        modules: global_modules,
    };
    if let Ok(json) = serde_json::to_string_pretty(&shared_config) {
        writes.push(WriteOp {
            path: dir.join(CONFIG_FILE),
            content: json.into_bytes(),
        });
    }

    // Chunked log files (global, shared across workers)
    writes.extend(crate::modules::logs::build_log_write_ops(&state.logs, state.next_log_id));

    // Build important_panel_uids
    let mut important_uids: HashMap<ContextType, String> = HashMap::new();
    for ctx in &state.context {
        let dominated = (ctx.context_type.is_fixed() || ctx.context_type == ContextType::Conversation)
            && ctx.context_type != ContextType::System && ctx.context_type != ContextType::Library;
        if dominated {
            if let Some(uid) = &ctx.uid {
                important_uids.insert(ctx.context_type, uid.clone());
            }
        }
    }

    // Build panel_uid_to_local_id (dynamic panels only — excludes fixed and Conversation)
    let panel_uid_to_local_id: HashMap<String, String> = state.context.iter()
        .filter(|c| c.uid.is_some() && !c.context_type.is_fixed() && c.context_type != ContextType::Conversation)
        .filter_map(|c| c.uid.as_ref().map(|uid| (uid.clone(), c.id.clone())))
        .collect();

    // WorkerState
    let worker_state = WorkerState {
        worker_id: DEFAULT_WORKER_ID.to_string(),
        important_panel_uids: important_uids,
        panel_uid_to_local_id,
        next_tool_id: state.next_tool_id,
        next_result_id: state.next_result_id,
        modules: worker_modules,
    };
    if let Ok(json) = serde_json::to_string_pretty(&worker_state) {
        writes.push(WriteOp {
            path: dir.join(crate::constants::STATES_DIR).join(format!("{}.json", DEFAULT_WORKER_ID)),
            content: json.into_bytes(),
        });
    }

    // Panels
    let panels_dir = dir.join(crate::constants::PANELS_DIR);
    let mut known_uids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for ctx in state.context.iter() {
        if ctx.context_type == ContextType::System || ctx.context_type == ContextType::Library {
            continue;
        }
        if let Some(uid) = &ctx.uid {
            known_uids.insert(uid.clone());
            let panel_data = PanelData {
                uid: uid.clone(),
                panel_type: ctx.context_type,
                name: ctx.name.clone(),
                token_count: ctx.token_count,
                last_refresh_ms: ctx.last_refresh_ms,
                message_uids: if ctx.context_type == ContextType::Conversation {
                    state.messages.iter()
                        .map(|m| m.uid.clone().unwrap_or_else(|| m.id.clone()))
                        .collect()
                } else if ctx.context_type == ContextType::ConversationHistory {
                    ctx.history_messages.as_ref()
                        .map(|msgs| msgs.iter().map(|m| m.uid.clone().unwrap_or_else(|| m.id.clone())).collect())
                        .unwrap_or_default()
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
                result_command: ctx.result_command.clone(),
                result_command_hash: ctx.result_command_hash.clone(),
                skill_prompt_id: ctx.skill_prompt_id.clone(),
                content_hash: ctx.content_hash.clone(),
                panel_total_cost: if ctx.panel_total_cost > 0.0 { Some(ctx.panel_total_cost) } else { None },
            };
            if let Ok(json) = serde_json::to_string_pretty(&panel_data) {
                writes.push(WriteOp {
                    path: panels_dir.join(format!("{}.json", uid)),
                    content: json.into_bytes(),
                });
            }
        }
    }

    // History messages for ConversationHistory panels
    let messages_dir = dir.join(crate::constants::MESSAGES_DIR);
    for ctx in &state.context {
        if ctx.context_type == ContextType::ConversationHistory {
            if let Some(ref msgs) = ctx.history_messages {
                for msg in msgs {
                    let file_id = msg.uid.as_ref().unwrap_or(&msg.id);
                    if let Ok(yaml) = serde_yaml::to_string(msg) {
                        writes.push(WriteOp {
                            path: messages_dir.join(format!("{}.yaml", file_id)),
                            content: yaml.into_bytes(),
                        });
                    }
                }
            }
        }
    }

    // Orphan panel deletion
    if let Ok(entries) = fs::read_dir(&panels_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if !known_uids.contains(stem) {
                    deletes.push(DeleteOp { path });
                }
            }
        }
    }

    WriteBatch { writes, deletes, ensure_dirs }
}

/// Build a WriteOp for a single message (CPU work only — no I/O).
pub fn build_message_op(msg: &Message) -> WriteOp {
    let dir = PathBuf::from(STORE_DIR).join(crate::constants::MESSAGES_DIR);
    let file_id = msg.uid.as_ref().unwrap_or(&msg.id);
    let yaml = serde_yaml::to_string(msg).unwrap_or_default();
    WriteOp {
        path: dir.join(format!("{}.yaml", file_id)),
        content: yaml.into_bytes(),
    }
}

/// Save state synchronously (blocking I/O on calling thread).
/// Used for shutdown paths and places where the PersistenceWriter is not available.
/// Prefer `build_save_batch` + `PersistenceWriter::send_batch` in the main event loop.
pub fn save_state(state: &State) {
    let batch = build_save_batch(state);
    // Execute synchronously
    for dir in &batch.ensure_dirs {
        let _ = fs::create_dir_all(dir);
    }
    for op in &batch.writes {
        if let Some(parent) = op.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&op.path, &op.content);
    }
    for op in &batch.deletes {
        let _ = fs::remove_file(&op.path);
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
