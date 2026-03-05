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

    if state.tools.is_empty() {
        state.tools = crate::modules::active_tool_definitions(&state.active_modules);
    }

    let _r = dotenvy::dotenv().ok();
    cp_mod_github::types::GithubState::get_mut(state).github_token = std::env::var("GITHUB_TOKEN").ok();

    set_active_theme(&state.active_theme);
}
