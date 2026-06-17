//! Phased boot helpers — module data extraction and per-module initialization.
//!
//! Split from `mod.rs` so the persistence module stays under the 500-line limit.
//! Called by `main.rs` during the boot loading screen.

use std::collections::HashMap;

use crate::infra::config::set_active_theme;
use crate::state::State;

use super::BootConfig;

/// Module data maps extracted from `BootConfig` before consumption.
/// Passed to `boot_init_modules` so main.rs can render per-module progress.
pub(crate) struct BootModuleData {
    /// Global module data (from `config::Shared.modules`)
    pub global: HashMap<String, serde_json::Value>,
    /// Worker module data (from `WorkerState.modules`)
    pub worker: HashMap<String, serde_json::Value>,
}

/// Extract module data maps from `BootConfig` before it is consumed by `boot_assemble_state`.
/// Returns the maps needed by `boot_init_modules`.
pub(crate) fn boot_extract_module_data(cfg: &BootConfig) -> BootModuleData {
    BootModuleData { global: cfg.shared.modules.clone(), worker: cfg.worker.modules.clone() }
}

/// Phase 5: Initialize all modules and load their persisted data.
///
/// Calls `progress(module_name)` before each module so the caller can
/// render per-module progress on the boot loading screen.
pub(crate) fn boot_init_modules(state: &mut State, module_data: &BootModuleData, mut progress: impl FnMut(&str)) {
    // Load .env files FIRST — modules read env vars during init_state
    // (e.g. DATALAB_API_KEY for OCR, GITHUB_TOKEN for gh).
    // Project-specific .env takes priority (dotenvy won't override existing).
    let _local = dotenvy::dotenv().ok();
    // Global .env as fallback (only sets vars not already present).
    if let Ok(home) = std::env::var("HOME") {
        let global_env = std::path::PathBuf::from(home).join(".context-pilot").join(".env");
        let _global = dotenvy::from_path(&global_env).ok();
    }

    // Pre-start heavy daemons in parallel — the biggest boot perf win.
    // Meilisearch and Console server start concurrently.
    // When module init_state() runs, each daemon is already healthy and
    // the reconnect path fires instantly.
    pre_start_daemons(&mut progress);

    let modules = crate::modules::all_modules();

    for module in &modules {
        progress(module.name());
        module.init_state(state);
    }

    let null = serde_json::Value::Null;
    for module in &modules {
        progress(module.name());
        let data = if module.is_global() {
            module_data.global.get(module.id()).unwrap_or(&null)
        } else {
            module_data.worker.get(module.id()).unwrap_or(&null)
        };
        module.load_module_data(data, state);

        let worker_data = module_data.worker.get(&format!("{}_worker", module.id())).unwrap_or(&null);
        module.load_worker_data(worker_data, state);
    }

    // Always rebuild the tool list so it includes the reverie's optimize_context
    // AND any runtime-discovered tools (e.g. MCP server tools connected during
    // module init).  Plain active_tool_definitions (called by overview's
    // load_module_data) misses dynamic tools; an unconditional rebuild picks them
    // up while preserving the disabled-tool state already applied by overview.
    crate::modules::rebuild_tools(state);

    cp_mod_github::types::GithubState::get_mut(state).github_token = std::env::var("GITHUB_TOKEN").ok();

    set_active_theme(&state.active_theme);
}

/// Pre-start the three heavy daemons in parallel threads.
///
/// Spawns Meilisearch and the Console server concurrently.  Each has
/// its own ~15 s health-check timeout.  Joining waits for
/// `max(startup₁, startup₂)` instead of the sequential sum.
///
/// Failures are logged but never halt boot — the normal module init
/// will retry the startup for any daemon that failed here.
fn pre_start_daemons(progress: &mut impl FnMut(&str)) {
    progress("pre-starting daemons");

    let meili_handle = std::thread::spawn(cp_mod_search::pre_start_daemon);
    let console_handle = std::thread::spawn(cp_mod_console::manager::find_or_create_server);

    // Join both — each thread has internal timeouts so this won't
    // block indefinitely.  Log results without aborting.
    for (name, result) in [("Meilisearch", meili_handle.join()), ("Console", console_handle.join())] {
        match result {
            Ok(Ok(())) => log::info!("Pre-start: {name} ready"),
            Ok(Err(e)) => log::warn!("Pre-start: {name} failed: {e}"),
            Err(_panic) => log::warn!("Pre-start: {name} thread panicked"),
        }
    }
}
