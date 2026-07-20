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

/// (global, worker) module-data maps keyed by module id.
type ModuleDataMaps = (HashMap<String, serde_json::Value>, HashMap<String, serde_json::Value>);

/// (`important_panel_uids` by kind, `panel_uid` → local id) worker maps.
type PanelUidMaps = (HashMap<Kind, String>, HashMap<String, String>);

/// Build global + per-worker module-data maps by polling every registered module.
fn build_module_data_maps(state: &State) -> ModuleDataMaps {
    let mut global_modules = HashMap::new();
    let mut worker_modules = HashMap::new();
    for module in crate::modules::all_modules() {
        let data = module.save_module_data(state);
        if !data.is_null() {
            if module.is_global() {
                let _r = global_modules.insert(module.id().to_owned(), data);
            } else {
                let _r = worker_modules.insert(module.id().to_owned(), data);
            }
        }
        let worker_data = module.save_worker_data(state);
        if !worker_data.is_null() {
            let _r = worker_modules.insert(format!("{}_worker", module.id()), worker_data);
        }
    }
    // Cache optimization engine (survives reloads via worker state)
    if let Some(json) = &state.cache_engine_json
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(json)
    {
        let _r = worker_modules.insert("cache_engine".to_owned(), val);
    }
    (global_modules, worker_modules)
}

/// The message UIDs (falling back to local id) for a panel's persisted body:
/// live conversation messages, or a history panel's frozen chunk, else empty.
fn panel_message_uids(ctx: &crate::state::Entry, state: &State) -> Vec<String> {
    if ctx.context_type.as_str() == Kind::CONVERSATION {
        state.messages.iter().map(|m| m.uid.clone().unwrap_or_else(|| m.id.clone())).collect()
    } else if ctx.context_type.as_str() == Kind::CONVERSATION_HISTORY {
        ctx.history_messages
            .as_ref()
            .map(|msgs: &Vec<Message>| msgs.iter().map(|m| m.uid.clone().unwrap_or_else(|| m.id.clone())).collect())
            .unwrap_or_default()
    } else {
        vec![]
    }
}

/// Emit one `{uid}.json` write op per persistable panel and record every seen UID
/// (used afterward to prune orphaned panel files).
fn build_panel_write_ops(
    state: &State,
    panels_dir: &std::path::Path,
    known_uids: &mut std::collections::HashSet<String>,
) -> Vec<WriteOp> {
    let mut writes = Vec::new();
    for ctx in &state.context {
        if ctx.context_type.as_str() == Kind::SYSTEM || ctx.context_type.as_str() == Kind::LIBRARY {
            continue;
        }
        let Some(uid) = &ctx.uid else { continue };
        let _r = known_uids.insert(String::clone(uid));
        let panel_data = PanelData::new(uid.clone(), ctx.context_type.clone(), ctx.name.clone())
            .with_metrics(ctx.token_count, ctx.last_refresh_ms)
            .with_message_uids(panel_message_uids(ctx, state))
            .with_metadata(ctx.metadata.clone(), ctx.content_hash.clone())
            .with_stats(
                (ctx.panel_total_cost > 0.0f64).then_some(ctx.panel_total_cost),
                ctx.total_freezes,
                ctx.total_cache_misses,
            );
        if let Ok(json) = serde_json::to_string_pretty(&panel_data) {
            writes.push(WriteOp { path: panels_dir.join(format!("{uid}.json")), content: json.into_bytes() });
        }
    }
    writes
}

/// Emit one `{uid}.yaml` write op per message held in a `ConversationHistory` panel.
fn build_history_message_ops(state: &State, messages_dir: &std::path::Path) -> Vec<WriteOp> {
    let mut writes = Vec::new();
    for ctx in &state.context {
        if ctx.context_type.as_str() == Kind::CONVERSATION_HISTORY
            && let Some(msgs) = &ctx.history_messages
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
    writes
}

/// Scan the panels dir and emit a delete op for every `{uid}.json` whose UID is
/// no longer live in `known_uids`.
fn collect_orphan_deletes(
    panels_dir: &std::path::Path,
    known_uids: &std::collections::HashSet<String>,
) -> Vec<DeleteOp> {
    let mut deletes = Vec::new();
    if let Ok(entries) = fs::read_dir(panels_dir) {
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
    deletes
}

/// Build `important_panel_uids` + `panel_uid_to_local_id` maps for the worker state.
fn build_panel_uid_maps(state: &State) -> PanelUidMaps {
    let mut important_uids: HashMap<Kind, String> = HashMap::new();
    for ctx in &state.context {
        let dominated = (ctx.context_type.is_fixed() || ctx.context_type.as_str() == Kind::CONVERSATION)
            && ctx.context_type.as_str() != Kind::SYSTEM
            && ctx.context_type.as_str() != Kind::LIBRARY;
        if dominated && let Some(uid) = &ctx.uid {
            let _r = important_uids.insert(ctx.context_type.clone(), String::clone(uid));
        }
    }
    let panel_uid_to_local_id: HashMap<String, String> = state
        .context
        .iter()
        .filter(|c| c.uid.is_some() && !c.context_type.is_fixed() && c.context_type.as_str() != Kind::CONVERSATION)
        .filter_map(|c| c.uid.as_ref().map(|uid: &String| (uid.clone(), c.id.clone())))
        .collect();
    (important_uids, panel_uid_to_local_id)
}

/// Serialize all config, worker state, panels, and history messages
/// into a batch of file write/delete operations.
pub(crate) fn build_save_batch(state: &State) -> WriteBatch {
    let _guard = crate::profile!("persist::build_save_batch");
    let _fg = cp_base::flame!("save_batch");
    let dir = PathBuf::from(STORE_DIR);
    let mut writes = Vec::new();
    let ensure_dirs = vec![
        dir.clone(),
        dir.join(crate::infra::constants::STATES_DIR),
        dir.join(crate::infra::constants::PANELS_DIR),
        dir.join(crate::infra::constants::MESSAGES_DIR),
        dir.join(cp_mod_logs::LOGS_DIR),
        dir.join(cp_mod_console::CONSOLE_DIR),
    ];

    let (global_modules, worker_modules) = build_module_data_maps(state);

    // Shared config
    let shared_config = SharedConfig::default()
        .with_active_theme(state.active_theme.clone())
        .with_owner_pid(Some(current_pid()))
        .with_ui(state.selected_context, state.input.clone(), state.input_cursor)
        .with_view_mode(state.view_mode)
        .with_modules(global_modules);
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

    let (important_uids, panel_uid_to_local_id) = build_panel_uid_maps(state);

    // WorkerState
    let worker_state = WorkerState::default()
        .with_worker_id(DEFAULT_WORKER_ID.to_owned())
        .with_panel_uids(important_uids, panel_uid_to_local_id)
        .with_id_counters(state.next_tool_id, state.next_result_id)
        .with_modules(worker_modules);
    if let Ok(json) = serde_json::to_string_pretty(&worker_state) {
        writes.push(WriteOp {
            path: dir.join(crate::infra::constants::STATES_DIR).join(format!("{DEFAULT_WORKER_ID}.json")),
            content: json.into_bytes(),
        });
    }

    // Panels + history messages + orphan pruning
    let panels_dir = dir.join(crate::infra::constants::PANELS_DIR);
    let messages_dir = dir.join(crate::infra::constants::MESSAGES_DIR);
    let mut known_uids: std::collections::HashSet<String> = std::collections::HashSet::new();
    writes.extend(build_panel_write_ops(state, &panels_dir, &mut known_uids));
    writes.extend(build_history_message_ops(state, &messages_dir));
    let deletes = collect_orphan_deletes(&panels_dir, &known_uids);

    WriteBatch { writes, deletes, ensure_dirs }
}

/// Build a `WriteOp` for a single message (CPU work only — no I/O).
pub(crate) fn build_message_op(msg: &Message) -> WriteOp {
    let dir = PathBuf::from(STORE_DIR).join(crate::infra::constants::MESSAGES_DIR);
    let file_id = msg.uid.as_ref().unwrap_or(&msg.id);
    let yaml = serde_yaml::to_string(msg).unwrap_or_default();
    WriteOp { path: dir.join(format!("{file_id}.yaml")), content: yaml.into_bytes() }
}

/// Execute one write op synchronously (create parent dir, then write).
fn exec_write_op(op: &WriteOp) {
    if let Some(parent) = op.path.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        drop(writeln!(std::io::stderr(), "[persistence] failed to create dir {}: {}", parent.display(), e));
        return;
    }
    if let Err(e) = fs::write(&op.path, &op.content) {
        drop(writeln!(std::io::stderr(), "[persistence] failed to write {}: {}", op.path.display(), e));
    }
}

/// Execute one delete op synchronously (ignoring not-found).
fn exec_delete_op(op: &DeleteOp) {
    if let Err(e) = fs::remove_file(&op.path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        drop(writeln!(std::io::stderr(), "[persistence] failed to delete {}: {}", op.path.display(), e));
    }
}

/// Save state synchronously (blocking I/O on calling thread).
/// Used for shutdown paths and places where the `PersistenceWriter` is not available.
/// Prefer `build_save_batch` + `PersistenceWriter::send_batch` in the main event loop.
pub(crate) fn save_state(state: &State) {
    let _fg = cp_base::flame!("save_state");
    let batch = build_save_batch(state);
    for dir in &batch.ensure_dirs {
        if let Err(e) = fs::create_dir_all(dir) {
            drop(writeln!(std::io::stderr(), "[persistence] failed to create dir {}: {}", dir.display(), e));
        }
    }
    for op in &batch.writes {
        exec_write_op(op);
    }
    for op in &batch.deletes {
        exec_delete_op(op);
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
