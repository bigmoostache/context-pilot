//! Persistence module for multi-worker state management
//!
//! This module handles the file-based persistence of:
//! - `config::Shared` (config.json) - Global settings shared across workers
//! - `WorkerState` (states/{worker}.json) - Worker-specific state
//! - `PanelData` (panels/{uid}.json) - Dynamic panel metadata
//! - Messages (messages/{uid}.yaml) - Conversation messages
mod boot;

pub(crate) use boot::{boot_extract_module_data, boot_init_modules};
pub(crate) mod config;
pub(crate) mod message;
pub(crate) mod panel;
pub(crate) mod worker;
pub(crate) mod writer;

// Re-export commonly used functions
pub(crate) use config::current_pid;
pub(crate) use message::{delete_message, load_message, save_message};
pub(crate) use writer::{DeleteOp, PersistenceWriter, WriteBatch, WriteOp};

use chrono::Local;
use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;

use cp_mod_logs::types::LogsState;

use crate::infra::config::set_active_theme;
use crate::infra::constants::{CONFIG_FILE, DEFAULT_WORKER_ID, STORE_DIR};
use crate::state::{Entry, Kind, Message, PanelData, SharedConfig, State, WorkerState};

/// Errors directory name
const ERRORS_DIR: &str = "errors";

/// Check if new multi-file format exists
fn new_format_exists() -> bool {
    PathBuf::from(STORE_DIR).join(CONFIG_FILE).exists()
}

// ─── Phased Boot Loading ────────────────────────────────────────────────────
// Split into phases so main.rs can render progress between each.

/// Phase 1 result: config + worker state loaded from disk.
pub(crate) struct BootConfig {
    /// Global shared configuration.
    pub shared: SharedConfig,
    /// Per-worker state.
    pub worker: WorkerState,
}

/// Phase 2 result: context panels + message UIDs to load next.
pub(crate) struct BootPanels {
    /// Loaded context elements (panels).
    pub context: Vec<Entry>,
    /// UIDs of conversation messages to load in phase 3.
    pub message_uids: Vec<String>,
    /// Total number of panels loaded from disk.
    pub panel_count: usize,
}

/// Phase 1: Load config.json and worker state from disk.
pub(crate) fn boot_load_config() -> BootConfig {
    let shared = config::load_config().unwrap_or_default();
    let worker = worker::load_worker(DEFAULT_WORKER_ID).unwrap_or_default();
    BootConfig { shared, worker }
}

/// Phase 2: Build context panels from panel JSONs on disk.
pub(crate) fn boot_load_panels(cfg: &BootConfig) -> BootPanels {
    let mut context: Vec<Entry> = Vec::new();
    let important = &cfg.worker.important_panel_uids;
    let mut panel_count: usize = 0;

    // Conversation panel
    if let Some(uid) = important.get(&Kind::new(Kind::CONVERSATION))
        && let Some(panel_data) = panel::load_panel(uid)
    {
        context.push(panel_to_context(&panel_data, "chat"));
        panel_count = panel_count.saturating_add(1);
    }

    // Fixed panels (P0-P7)
    let defaults = crate::modules::all_fixed_panel_defaults();
    for (pos, d) in defaults.iter().enumerate() {
        let id = format!("P{pos}");
        if d.context_type.as_str() == Kind::SYSTEM {
            context.push(crate::modules::make_default_entry(
                &id,
                d.context_type.clone(),
                d.display_name,
                d.cache_deprecated,
            ));
        } else if let Some(uid) = important.get(&d.context_type)
            && let Some(panel_data) = panel::load_panel(uid)
        {
            context.push(panel_to_context(&panel_data, &id));
            panel_count = panel_count.saturating_add(1);
        }
    }

    // Dynamic panels (P8+)
    let mut dynamic_panels: Vec<(String, Entry)> = cfg
        .worker
        .panel_uid_to_local_id
        .iter()
        .filter_map(|(uid, local_id)| {
            panel::load_panel(uid).map(|p| {
                let mut elem = panel_to_context(&p, local_id);

                if p.panel_type.as_str() == Kind::CONVERSATION_HISTORY && !p.message_uids.is_empty() {
                    let msgs: Vec<Message> =
                        p.message_uids.iter().filter_map(|msg_uid| load_message(msg_uid)).collect();
                    if !msgs.is_empty() {
                        let chunk_text = crate::state::format_messages_to_chunk(&msgs);
                        let token_count = crate::state::estimate_tokens(&chunk_text);
                        let total_pages = crate::state::compute_total_pages(token_count);
                        elem.cached_content = Some(chunk_text);
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
    dynamic_panels.sort_by(|a, b| {
        let a_num: usize = a.0.trim_start_matches('P').parse().unwrap_or(999);
        let b_num: usize = b.0.trim_start_matches('P').parse().unwrap_or(999);
        a_num.cmp(&b_num)
    });
    panel_count = panel_count.saturating_add(dynamic_panels.len());
    for (_, elem) in dynamic_panels {
        context.push(elem);
    }

    // Extract message UIDs for Phase 3
    let message_uids: Vec<String> = important
        .get(&Kind::new(Kind::CONVERSATION))
        .and_then(|uid| panel::load_panel(uid))
        .map(|p| p.message_uids)
        .unwrap_or_default();

    BootPanels { context, message_uids, panel_count }
}

/// Phase 3: Load conversation messages from individual YAML files.
pub(crate) fn boot_load_messages(uids: &[String]) -> Vec<Message> {
    uids.iter().filter_map(|uid| load_message(uid)).collect()
}

/// Phase 4: Assemble final `State` from boot phases.
pub(crate) fn boot_assemble_state(cfg: BootConfig, panels: BootPanels, messages: Vec<Message>) -> State {
    // Calculate display ID counters from loaded messages
    let next_user_id = messages
        .iter()
        .filter(|m| m.id.starts_with('U'))
        .filter_map(|m| m.id.get(1..).unwrap_or("").parse::<usize>().ok())
        .max()
        .map_or(1, |n| n.saturating_add(1));
    let next_assistant_id = messages
        .iter()
        .filter(|m| m.id.starts_with('A'))
        .filter_map(|m| m.id.get(1..).unwrap_or("").parse::<usize>().ok())
        .max()
        .map_or(1, |n| n.saturating_add(1));

    // Module init + data loading is driven by main.rs via boot_init_modules()
    // so it can render per-module progress on the loading screen.
    State {
        context: panels.context,
        messages,
        selected_context: cfg.shared.selected_context,
        next_user_id,
        next_assistant_id,
        next_tool_id: cfg.worker.next_tool_id,
        next_result_id: cfg.worker.next_result_id,
        input: cfg.shared.draft_input,
        input_cursor: cfg.shared.draft_cursor,
        sidebar_mode: cfg.shared.sidebar_mode,
        active_theme: cfg.shared.active_theme,
        ..State::default()
    }
}

// ─── Legacy Entry Point ─────────────────────────────────────────────────────

/// Load state: delegates to phased boot for existing projects, or creates fresh defaults.
pub(crate) fn load_state() -> State {
    if new_format_exists() {
        // Existing project — use phased boot (monolithic path for non-TUI callers)
        let cfg = boot_load_config();
        let module_data = boot_extract_module_data(&cfg);
        let panels = boot_load_panels(&cfg);
        let messages = boot_load_messages(&panels.message_uids);
        let mut state = boot_assemble_state(cfg, panels, messages);
        boot_init_modules(&mut state, &module_data, |_| {});
        state
    } else {
        // Fresh start - create default state
        let mut state = State::default();
        state.active_modules = crate::modules::default_active_modules();
        state.tools = crate::modules::active_tool_definitions(&state.active_modules);
        state.tools.push(crate::app::reverie::tools::optimize_context_tool_definition());
        for module in crate::modules::all_modules() {
            module.init_state(&mut state);
        }
        set_active_theme(&state.active_theme);
        state
    }
}

/// Convert `PanelData` to `Entry`
fn panel_to_context(panel: &PanelData, local_id: &str) -> Entry {
    Entry {
        id: local_id.to_string(),
        uid: Some(panel.uid.clone()),
        context_type: panel.panel_type.clone(),
        name: panel.name.clone(),
        token_count: panel.token_count,
        metadata: panel.metadata.clone(),
        cached_content: None,
        history_messages: None,
        cache_deprecated: true, // Will be refreshed on load
        cache_in_flight: false,
        // Use saved timestamp if available, otherwise current time for new panels
        last_refresh_ms: if panel.last_refresh_ms > 0 { panel.last_refresh_ms } else { crate::app::panels::now_ms() },
        content_hash: panel.content_hash.clone(),
        source_hash: None,
        current_page: 0,
        total_pages: 1,
        full_token_count: 0,
        panel_cache_hit: false,
        panel_total_cost: panel.panel_total_cost.unwrap_or(0.0),
        freeze_count: 0,
        total_freezes: 0,
        total_cache_misses: 0,
        last_emitted_content: None,
        last_emitted_hash: None,
        last_emitted_context: None,
    }
}

/// Convert `PanelData` to `Entry`
/// This serializes all config, worker state, panels, and history messages
/// into a batch of file write/delete operations.
pub(crate) fn build_save_batch(state: &State) -> WriteBatch {
    let _guard = crate::profile!("persist::build_save_batch");
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

    // Shared config
    let shared_config = SharedConfig {
        schema_version: crate::state::config::SCHEMA_VERSION,
        reload_requested: false,
        active_theme: state.active_theme.clone(),
        owner_pid: Some(current_pid()),
        selected_context: state.selected_context,
        draft_input: state.input.clone(),
        draft_cursor: state.input_cursor,
        sidebar_mode: state.sidebar_mode,
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

/// Check if we still own the state file (another instance may have taken over)
/// Returns false if another process has claimed ownership
pub(crate) fn check_ownership() -> bool {
    if let Some(cfg) = config::load_config()
        && let Some(owner) = cfg.owner_pid
    {
        return owner == current_pid();
    }
    // If we can't read the file or there's no owner, assume we're still the owner
    true
}

/// Log an error to .context-pilot/errors/ and return the file path
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
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
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
