//! Phased boot helpers — module data extraction and per-module initialization.
//!
//! Split from `mod.rs` so the persistence module stays under the 500-line limit.
//! Called by `main.rs` during the boot loading screen.

use std::collections::HashMap;

use cp_base::state::runtime::bundle::ThreadRuntime;

use crate::infra::config::set_active_theme;
use crate::state::{Message, SharedConfig, State};

use super::{BootConfig, boot_load_messages, boot_load_panels};

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

/// Merge the `.env` files into the process environment - project-local first,
/// then `~/.context-pilot/.env` overriding it (that is where the settings page
/// writes, so it carries the latest user intent). Override mode, so file
/// values always win over stale shell values inherited from a parent.
fn load_dotenv_files() {
    let _local = dotenvy::dotenv_override().ok();
    if let Some(home) = std::env::var_os("HOME") {
        let global_env = std::path::PathBuf::from(home).join(".context-pilot").join(".env");
        let _global = dotenvy::from_path_override(&global_env).ok();
    }
}

/// Environment preflight - the very first thing `main` does, before the
/// logger, the terminal or any module: merge the `.env` files, validate every
/// variable strictly (see `docs/ENV.md`) and install the typed result. On
/// failure the aggregated report is returned for `main` to print.
///
/// # Errors
///
/// The rendered report when the environment is invalid.
pub(crate) fn preflight_env() -> Result<(), String> {
    load_dotenv_files();
    let loaded = cp_env::load(cp_env::spec::Target::Agent).map_err(|report| report.to_string())?;
    if cp_env::install(loaded.env) { Ok(()) } else { Err("environment installed twice".to_owned()) }
}

/// `cpilot --check-env`: the same merge and validation as a boot, rendered
/// for the terminal, with the exit status to use.
pub(crate) fn check_env() -> (String, bool) {
    load_dotenv_files();
    cp_env::check(cp_env::spec::Target::Agent)
}

/// Trigger vault initialization (the `.env` files were merged by
/// [`preflight_env`]) and warn about missing credentials before any module
/// tries to use them.
fn load_boot_env_and_check_vault() {
    for def in cp_vault::vault().health() {
        log::warn!("Missing credential: {} ({})", def.display, def.env_var);
    }
}

/// Load persisted per-module data (global or worker map) plus the `_worker`
/// suffix map into `state` for one module.
fn load_one_module_data(module: &dyn crate::modules::Module, module_data: &BootModuleData, state: &mut State) {
    let null = serde_json::Value::Null;
    let data = if module.is_global() {
        module_data.global.get(module.id()).unwrap_or(&null)
    } else {
        module_data.worker.get(module.id()).unwrap_or(&null)
    };
    // Route first-inserts into the correct scope map: a module's main state
    // follows its is_global(); its worker-data slice is always per-thread.
    state.set_init_scope(Some(module.is_global()));
    module.load_module_data(data, state);

    let worker_data = module_data.worker.get(&format!("{}_worker", module.id())).unwrap_or(&null);
    state.set_init_scope(Some(false));
    module.load_worker_data(worker_data, state);
    state.set_init_scope(None);
}

/// Phase 5: Initialize all modules and load their persisted data.
///
/// Calls `progress(module_name)` before each module so the caller can
/// render per-module progress on the boot loading screen.
pub(crate) fn boot_init_modules(state: &mut State, module_data: &BootModuleData, mut progress: impl FnMut(&str)) {
    load_boot_env_and_check_vault();

    // Pre-start heavy daemons in parallel — the biggest boot perf win.
    // Meilisearch and Console server start concurrently.
    // When module init_state() runs, each daemon is already healthy and
    // the reconnect path fires instantly.
    pre_start_daemons(&mut progress);

    let modules = crate::modules::all_modules();

    for module in &modules {
        progress(module.name());
        state.set_init_scope(Some(module.is_global()));
        module.init_state(state);
    }
    state.set_init_scope(None);

    for module in &modules {
        progress(module.name());
        load_one_module_data(module.as_ref(), module_data, state);
    }

    if state.tools.is_empty() {
        state.tools = crate::modules::active_tool_definitions(&state.active_modules);
    }

    cp_mod_github::types::GithubState::get_mut(state).github_token =
        cp_vault::vault().get("github").map(|s| s.expose().to_owned());

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

/// Initialise and load ONLY the per-thread (`is_global() == false`) modules into
/// a throwaway background-thread `State`, from that thread's persisted worker
/// module map — the per-thread half of [`boot_init_modules`], for
/// [`boot_load_thread_runtime`].
///
/// Global modules are deliberately skipped: they are fleet-shared singletons
/// that already live in the focused (resident) state's `shared_module_data`;
/// re-initialising them here would create a second, discarded copy. Only the
/// per-thread module data (spine inbox, queue, console ownership, watcher
/// registry, search/git views, …) is built, so the subsequent
/// [`ThreadRuntime::swap_with`] carries exactly that thread's per-thread state
/// out. Mirrors [`load_one_module_data`]'s per-thread branch (two passes: all
/// inits, then all loads) restricted to non-global modules.
fn boot_init_thread_modules(state: &mut State, worker_modules: &HashMap<String, serde_json::Value>) {
    let null = serde_json::Value::Null;
    let modules = crate::modules::all_modules();
    for module in &modules {
        if module.is_global() {
            continue;
        }
        state.set_init_scope(Some(false));
        module.init_state(state);
    }
    for module in &modules {
        if module.is_global() {
            continue;
        }
        let data = worker_modules.get(module.id()).unwrap_or(&null);
        state.set_init_scope(Some(false));
        module.load_module_data(data, state);

        let worker_data = worker_modules.get(&format!("{}_worker", module.id())).unwrap_or(&null);
        state.set_init_scope(Some(false));
        module.load_worker_data(worker_data, state);
    }
    state.set_init_scope(None);
}

/// Load one background thread's persisted per-thread context into a
/// [`ThreadRuntime`] (the registry payload for a non-focused thread), or `None`
/// when the thread has no `states/<thread_id>.json` yet (a *cold* thread — e.g.
/// one never advanced since the per-thread save landed, or a brand-new thread;
/// it boots empty and the reconcile pass gives it a fresh runtime).
///
/// It reuses the focused boot loaders ([`boot_load_panels`] /
/// [`boot_load_messages`], which read only `cfg.worker`, ignoring `cfg.shared`)
/// to rebuild the thread's panels + conversation, then assembles a throwaway
/// background `State` and [`swap_with`](ThreadRuntime::swap_with)s its per-thread
/// fields out into the returned runtime. Shared UI fields (draft input, selected
/// panel) are deliberately left at their defaults — they are persisted per-agent
/// in `config.json`, not per-thread, so a background thread must not inherit the
/// focused thread's draft.
pub(crate) fn boot_load_thread_runtime(thread_id: &str, next_uid: &mut usize) -> Option<ThreadRuntime> {
    let worker = super::worker::load_worker(thread_id)?;
    // `shared` is unused by the panel/message loaders — a default is enough.
    let cfg = BootConfig { shared: SharedConfig::default(), worker };
    let panels = boot_load_panels(&cfg);
    let messages = boot_load_messages(&panels.message_uids);

    // Display-id counters derived from the loaded messages (same rule as
    // `boot_assemble_state`); tool/result counters come from the worker file.
    let (next_user_id, next_assistant_id) = message_id_counters(&messages);
    let cache_engine_json = cfg.worker.modules.get("cache_engine").and_then(|v| serde_json::to_string(v).ok());

    let mut bg = State::default()
        .with_context(panels.context)
        .with_messages(messages)
        .with_id_counters((next_user_id, next_assistant_id, cfg.worker.next_tool_id, cfg.worker.next_result_id))
        .with_cache_engine_json(cache_engine_json);
    boot_init_thread_modules(&mut bg, &cfg.worker.modules);
    // Ensure this thread owns the fixed base panels (Todo, Overview, Memory,
    // …) exactly like the focused thread does at boot — otherwise drilling into
    // a background thread shows a panel-less view. Idempotent: only MISSING
    // fixed panels are added; persisted ones keep their UIDs. UIDs are minted
    // from the SHARED counter so a background thread's conversation panel can't
    // collide with another thread's on `panels/<uid>.json`.
    ensure_thread_fixed_panels(&mut bg, next_uid);

    let mut runtime = ThreadRuntime::new();
    runtime.swap_with(&mut bg); // runtime now holds this thread's per-thread context
    Some(runtime)
}

/// Build a [`ThreadRuntime`] for a **cold** thread — one in the roster with no
/// persisted `states/<tid>.json` (brand-new, or never advanced since per-thread
/// saves began).
///
/// Unlike [`boot_load_thread_runtime`], there is nothing on disk to load: the
/// conversation, panels and counters start empty. The one thing that must NOT
/// be empty is the per-thread **module map**. A bare
/// [`ThreadRuntime::new()`](ThreadRuntime::new) carries an empty
/// `thread_module_data`, so the moment the scheduler promotes the thread, swaps
/// it in and steps it, `check_spine`'s first `ext::<SpineState>()` would panic
/// with "module state not initialized". This runs the same per-thread module
/// init as a disk load ([`boot_init_thread_modules`] over an empty map) so the
/// swapped-in state is fully formed.
pub(crate) fn fresh_thread_runtime(next_uid: &mut usize) -> ThreadRuntime {
    let mut bg = State::default();
    boot_init_thread_modules(&mut bg, &HashMap::new());
    // A brand-new thread also needs its fixed base panels (Todo, Overview,
    // Memory, …) created up front, so switching into it shows the normal
    // panel-centric view rather than an empty one. UIDs are minted from the
    // shared counter (fleet-wide uniqueness for `panels/<uid>.json`).
    ensure_thread_fixed_panels(&mut bg, next_uid);
    let mut runtime = ThreadRuntime::new();
    runtime.swap_with(&mut bg);
    runtime
}

/// Create the fixed base panels (conversation + Todo/Overview/Memory/… at
/// P1..P9) on a background thread's throwaway `State`, minting any panel UIDs
/// from the fleet-wide `next_uid` counter rather than the throwaway state's
/// own (which starts at 0 and would collide across threads on
/// `panels/<uid>.json`).
///
/// Delegates to [`ensure_default_contexts`](crate::app::ensure_default_contexts)
/// — the same routine the focused thread uses at boot — after seeding the
/// throwaway state's UID cursor from the shared counter, then writes the
/// advanced cursor back so the next thread keeps minting unique UIDs.
fn ensure_thread_fixed_panels(bg: &mut State, next_uid: &mut usize) {
    bg.global_next_uid = *next_uid;
    crate::app::ensure_default_contexts(bg);
    *next_uid = bg.global_next_uid;
}

/// Next `(user, assistant)` display-id counters derived from a message list —
/// the max numeric suffix of each role's ids, plus one (defaulting to 1).
///
/// Shared by [`boot_load_thread_runtime`] and mirrors `boot_assemble_state`'s
/// counter derivation so a background thread numbers new messages exactly like
/// the focused thread would.
fn message_id_counters(messages: &[Message]) -> (usize, usize) {
    let next = |prefix: char| {
        messages
            .iter()
            .filter(|m| m.id.starts_with(prefix))
            .filter_map(|m| m.id.get(1..).unwrap_or("").parse::<usize>().ok())
            .max()
            .map_or(1, |n| n.saturating_add(1))
    };
    (next('U'), next('A'))
}
