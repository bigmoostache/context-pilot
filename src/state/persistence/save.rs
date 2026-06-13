//! Save operations: batch building, synchronous persistence, and utility functions.

use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;

use cp_mod_logs::types::LogsState;

use crate::infra::constants::{CONFIG_FILE, DEFAULT_WORKER_ID, STORE_DIR};
use crate::state::{Kind, Message, PanelData, SharedConfig, State, WorkerState};

use super::config::current_pid;
use super::writer::{DeleteOp, WriteBatch, WriteOp};

/// Errors directory name
const ERRORS_DIR: &str = "errors";

/// Serialize all config, worker state, panels, and history messages
/// into a batch of file write/delete operations.
pub(crate) fn build_save_batch(state: &State) -> WriteBatch {
    let _guard = crate::profile!("persist::build_save_batch");
    let _fg = cp_base::flame!("save_batch");
    let dir = PathBuf::from(STORE_DIR);
    let mut writes = Vec::new();
    let mut deletes = Vec::new();
    let ensure_dirs = vec![
        dir.clone(),
        dir.join(crate::infra::constants::STATES_DIR),
        dir.join(crate::infra::constants::PANELS_DIR),
        dir.join(crate::infra::constants::MESSAGES_DIR),
        dir.join(cp_mod_logs::LOGS_DIR),
        dir.join(cp_mod_console::CONSOLE_DIR),
    ];

    // Build module data maps
    let mut global_modules = HashMap::new();
    let mut worker_modules = HashMap::new();
    for module in crate::modules::all_modules() {
        let data = module.save_module_data(state);
        if !data.is_null() {
            if module.is_global() {
                let _r = global_modules.insert(module.id().to_string(), data);
            } else {
                let _r = worker_modules.insert(module.id().to_string(), data);
            }
        }
        let worker_data = module.save_worker_data(state);
        if !worker_data.is_null() {
            let _r = worker_modules.insert(format!("{}_worker", module.id()), worker_data);
        }
    }

    // Cache optimization engine (survives reloads via worker state)
    if let Some(ref json) = state.cache_engine_json
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(json)
    {
        let _r = worker_modules.insert("cache_engine".to_string(), val);
    }

    // Shared config
    let shared_config = SharedConfig {
        schema_version: crate::state::config::SCHEMA_VERSION,
        reload_requested: false,
        active_theme: state.active_theme.clone(),
        owner_pid: Some(current_pid()),
        selected_context: state.selected_context,
        draft_input: state.input.clone(),
        draft_cursor: state.input_cursor,
        view_mode: state.view_mode,
        modules: global_modules,
    };
    if let Ok(json) = serde_json::to_string_pretty(&shared_config) {
        writes.push(WriteOp { path: dir.join(CONFIG_FILE), content: json.into_bytes() });
    }

    // Chunked log files (global, shared across workers)
    let logs_state = LogsState::get(state);
    writes.extend(
        cp_mod_logs::build_log_write_ops(&logs_state.logs, logs_state.next_log_id)
            .into_iter()
            .map(|(path, content)| WriteOp { path, content }),
    );

    // Build important_panel_uids
    let mut important_uids: HashMap<Kind, String> = HashMap::new();
    for ctx in &state.context {
        let dominated = (ctx.context_type.is_fixed() || ctx.context_type.as_str() == Kind::CONVERSATION)
            && ctx.context_type.as_str() != Kind::SYSTEM
            && ctx.context_type.as_str() != Kind::LIBRARY;
        if dominated && let Some(uid) = &ctx.uid {
            let _r = important_uids.insert(ctx.context_type.clone(), String::clone(uid));
        }
    }

    // Build panel_uid_to_local_id (dynamic panels only — excludes fixed and Conversation)
    let panel_uid_to_local_id: HashMap<String, String> = state
        .context
        .iter()
        .filter(|c| c.uid.is_some() && !c.context_type.is_fixed() && c.context_type.as_str() != Kind::CONVERSATION)
        .filter_map(|c| c.uid.as_ref().map(|uid: &String| (uid.clone(), c.id.clone())))
        .collect();

    // WorkerState
    let worker_state = WorkerState {
        schema_version: crate::state::config::SCHEMA_VERSION,
        worker_id: DEFAULT_WORKER_ID.to_string(),
        important_panel_uids: important_uids,
        panel_uid_to_local_id,
        next_tool_id: state.next_tool_id,
        next_result_id: state.next_result_id,
        modules: worker_modules,
    };
    if let Ok(json) = serde_json::to_string_pretty(&worker_state) {
        writes.push(WriteOp {
            path: dir.join(crate::infra::constants::STATES_DIR).join(format!("{DEFAULT_WORKER_ID}.json")),
            content: json.into_bytes(),
        });
    }

    // Panels
    let panels_dir = dir.join(crate::infra::constants::PANELS_DIR);
    let mut known_uids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for ctx in &state.context {
        if ctx.context_type.as_str() == Kind::SYSTEM || ctx.context_type.as_str() == Kind::LIBRARY {
            continue;
        }
        if let Some(uid) = &ctx.uid {
            let _r = known_uids.insert(String::clone(uid));
            let panel_data = PanelData {
                uid: uid.clone(),
                panel_type: ctx.context_type.clone(),
                name: ctx.name.clone(),
                token_count: ctx.token_count,
                last_refresh_ms: ctx.last_refresh_ms,
                message_uids: if ctx.context_type.as_str() == Kind::CONVERSATION {
                    state.messages.iter().map(|m| m.uid.clone().unwrap_or_else(|| m.id.clone())).collect()
                } else if ctx.context_type.as_str() == Kind::CONVERSATION_HISTORY {
                    ctx.history_messages
                        .as_ref()
                        .map(|msgs: &Vec<Message>| {
                            msgs.iter().map(|m| m.uid.clone().unwrap_or_else(|| m.id.clone())).collect()
                        })
                        .unwrap_or_default()
                } else {
                    vec![]
                },
                metadata: ctx.metadata.clone(),
                content_hash: ctx.content_hash.clone(),
                panel_total_cost: (ctx.panel_total_cost > 0.0).then_some(ctx.panel_total_cost),
                total_freezes: ctx.total_freezes,
                total_cache_misses: ctx.total_cache_misses,
            };
            if let Ok(json) = serde_json::to_string_pretty(&panel_data) {
                writes.push(WriteOp { path: panels_dir.join(format!("{uid}.json")), content: json.into_bytes() });
            }
        }
    }

    // History messages for ConversationHistory panels
    let messages_dir = dir.join(crate::infra::constants::MESSAGES_DIR);
    for ctx in &state.context {
        if ctx.context_type.as_str() == Kind::CONVERSATION_HISTORY
            && let Some(ref msgs) = ctx.history_messages
        {
            for msg in msgs {
                let file_id = msg.uid.as_ref().unwrap_or(&msg.id);
                if let Ok(yaml) = serde_yaml::to_string(msg) {
                    writes.push(WriteOp {
                        path: messages_dir.join(format!("{file_id}.yaml")),
                        content: yaml.into_bytes(),
                    });
                }
            }
        }
    }

    // Orphan panel deletion
    if let Ok(entries) = fs::read_dir(&panels_dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                && !known_uids.contains(stem)
            {
                deletes.push(DeleteOp { path });
            }
        }
    }

    WriteBatch { writes, deletes, ensure_dirs }
}

/// Build a `WriteOp` for a single message (CPU work only — no I/O).
pub(crate) fn build_message_op(msg: &Message) -> WriteOp {
    let dir = PathBuf::from(STORE_DIR).join(crate::infra::constants::MESSAGES_DIR);
    let file_id = msg.uid.as_ref().unwrap_or(&msg.id);
    let yaml = serde_yaml::to_string(msg).unwrap_or_default();
    WriteOp { path: dir.join(format!("{file_id}.yaml")), content: yaml.into_bytes() }
}

/// Save state synchronously (blocking I/O on calling thread).
/// Used for shutdown paths and places where the `PersistenceWriter` is not available.
/// Prefer `build_save_batch` + `PersistenceWriter::send_batch` in the main event loop.
pub(crate) fn save_state(state: &State) {
    let _fg = cp_base::flame!("save_state");
    let batch = build_save_batch(state);
    // Execute synchronously
    for dir in &batch.ensure_dirs {
        if let Err(e) = fs::create_dir_all(dir) {
            drop(writeln!(std::io::stderr(), "[persistence] failed to create dir {}: {}", dir.display(), e));
        }
    }
    for op in &batch.writes {
        if let Some(parent) = op.path.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            drop(writeln!(std::io::stderr(), "[persistence] failed to create dir {}: {}", parent.display(), e));
            continue;
        }
        if let Err(e) = fs::write(&op.path, &op.content) {
            drop(writeln!(std::io::stderr(), "[persistence] failed to write {}: {}", op.path.display(), e));
        }
    }
    for op in &batch.deletes {
        if let Err(e) = fs::remove_file(&op.path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            drop(writeln!(std::io::stderr(), "[persistence] failed to delete {}: {}", op.path.display(), e));
        }
    }
}

/// Check if we still own the state file (another instance may have taken over).
/// Returns false if another process has claimed ownership.
pub(crate) fn check_ownership() -> bool {
    if let Some(cfg) = super::config::load_config()
        && let Some(owner) = cfg.owner_pid
    {
        return owner == current_pid();
    }
    // If we can't read the file or there's no owner, assume we're still the owner
    true
}

/// Log an error to .context-pilot/errors/ and return the file path.
pub(crate) fn log_error(error: &str) -> String {
    let errors_dir = PathBuf::from(STORE_DIR).join(ERRORS_DIR);
    let _mkdir = fs::create_dir_all(&errors_dir).ok();

    // Count existing error files to determine next number
    let error_count = fs::read_dir(&errors_dir).map_or(0, |entries| {
        entries.filter_map(Result::ok).filter(|e| e.path().extension().is_some_and(|ext| ext == "txt")).count()
    });

    let error_num = error_count.saturating_add(1);
    let filename = format!("error_{error_num}.txt");
    let filepath = errors_dir.join(&filename);

    // Create error log content with timestamp
    let timestamp = cp_mod_utilities::time::now_local_ymd_hms();
    let content = format!(
        "Error Log #{error_num}\n\
         Timestamp: {timestamp}\n\
         \n\
         Error Details:\n\
         {error}\n"
    );

    let _r = fs::write(&filepath, content).ok();

    filepath.to_string_lossy().to_string()
}
