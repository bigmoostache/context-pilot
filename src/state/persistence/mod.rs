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
pub(crate) mod save;
pub(crate) mod worker;
pub(crate) mod writer;

// Re-export commonly used functions
pub(crate) use message::{delete_message, load_message, save_message};
pub(crate) use save::{build_message_op, build_save_batch, check_ownership, log_error, save_state};
pub(crate) use writer::PersistenceWriter;

use std::path::PathBuf;

use crate::infra::config::set_active_theme;
use crate::infra::constants::{CONFIG_FILE, DEFAULT_WORKER_ID, STORE_DIR};
use crate::state::{Entry, Kind, Message, PanelData, SharedConfig, State, WorkerState};

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

    // Restore cache engine state from worker modules
    let cache_engine_json = cfg.worker.modules.get("cache_engine").and_then(|v| serde_json::to_string(v).ok());

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
        view_mode: cfg.shared.view_mode,
        active_theme: cfg.shared.active_theme,
        cache_engine_json,
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
        scroll_state: cp_base::state::context::ScrollState::default(),
        panel_cache_hit: false,
        panel_total_cost: panel.panel_total_cost.unwrap_or(0.0),
        freeze_count: 0,
        total_freezes: 0,
        total_cache_misses: 0,
        emitted: cp_base::state::context::EmittedState::default(),
    }
}
