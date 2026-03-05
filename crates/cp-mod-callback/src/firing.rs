//! Callback firing logic: spawn scripts, register watchers.
//!
//! Separated from trigger.rs which handles file collection and pattern matching.

// Queue ID test marker — delete me later
use cp_base::config::constants;
use cp_base::panels::now_ms;
use cp_base::panels::time_arith::ms_to_secs;
use cp_base::state::runtime::State;
use cp_base::state::watchers::{DeferredPanel, Watcher, WatcherRegistry, WatcherResult};

use cp_mod_console::manager::SessionHandle;
use cp_mod_console::types::ConsoleState;

use crate::trigger::{MatchedCallback, build_changed_files_env};

/// Fire a single callback by spawning its script via the console server.
/// Creates a console session + watcher (no panel — deferred until failure).
///
/// For global callbacks, `single_file` is `None` and `$CP_CHANGED_FILES` contains all files.
/// For local callbacks, `single_file` is `Some(path)` and `$CP_CHANGED_FILE` contains that one file.
///
/// # Errors
///
/// Returns `Err(message)` if the script fails to execute or times out.
pub fn fire_callback(
    state: &mut State,
    matched: &MatchedCallback,
    blocking_tool_use_id: Option<&str>,
    single_file: Option<&str>,
) -> Result<String, String> {
    let def = &matched.definition;

    // Build the command with env vars baked in
    // Global: CP_CHANGED_FILES (plural, all matched files)
    // Local:  CP_CHANGED_FILE  (singular, one file per invocation)
    let (env_key, env_val) = single_file.map_or_else(
        || ("CP_CHANGED_FILES", build_changed_files_env(&matched.matched_files)),
        |file| ("CP_CHANGED_FILE", file.to_string()),
    );
    let project_root = std::env::current_dir().unwrap_or_default().to_string_lossy().to_string();

    // Use the callback's cwd if set, otherwise project root
    let cwd = def.cwd.clone().or_else(|| Some(project_root.clone()));

    // Build the script path — uses constants::STORE_DIR for scripts dir
    // For built-in callbacks, use the built_in_command directly instead of a script file.
    let command = if def.built_in {
        let base_cmd = def.built_in_command.as_deref().unwrap_or("echo 'no built_in_command set'");
        format!(
            "{env_key}={changed} CP_PROJECT_ROOT={root} CP_CALLBACK_NAME={name} {cmd}",
            changed = shell_escape(&env_val),
            root = shell_escape(&project_root),
            name = shell_escape(&def.name),
            cmd = base_cmd,
        )
    } else {
        let scripts_dir = std::path::PathBuf::from(constants::STORE_DIR).join("scripts");
        let script_path = scripts_dir.join(format!("{}.sh", def.name));
        let script_path_str = if script_path.is_absolute() {
            script_path.to_string_lossy().to_string()
        } else {
            format!("{}/{}", project_root, script_path.to_string_lossy())
        };

        // Check script exists and is readable before spawning
        if !script_path.exists() {
            return Err(format!("Callback '{}' script not found: {}", def.name, script_path.display(),));
        }

        format!(
            "{env_key}={changed} CP_PROJECT_ROOT={root} CP_CALLBACK_NAME={name} bash {script}",
            changed = shell_escape(&env_val),
            root = shell_escape(&project_root),
            name = shell_escape(&def.name),
            script = shell_escape(&script_path_str),
        )
    };

    // Generate session key via console state
    let session_key = {
        let cs = ConsoleState::get_mut(state);
        let key = format!("cb_{}", cs.next_session_id);
        cs.next_session_id = cs.next_session_id.saturating_add(1);
        key
    };

    // Spawn the process
    let handle = SessionHandle::spawn(session_key.clone(), command.clone(), cwd)?;

    // Store handle in console state (NO panel created — deferred until failure/timeout)
    let cs = ConsoleState::get_mut(state);
    drop(cs.sessions.insert(session_key.clone(), handle));

    // Register watcher
    let is_blocking = def.blocking && blocking_tool_use_id.is_some();
    let now = now_ms();
    let deadline_ms = def.timeout_secs.map(|t| now.saturating_add(t.saturating_mul(1000)));

    let watcher_desc = if is_blocking {
        format!("⏳ Callback '{}' (blocking)", def.name)
    } else {
        format!("👁 Callback '{}'", def.name)
    };

    let watcher = CallbackWatcher {
        watcher_id: format!("callback_{}_{}", def.id, session_key),
        session_name: session_key.clone(),
        callback_name: def.name.clone(),
        callback_tag: Box::leak(format!("callback_{}", def.id).into_boxed_str()),
        success_message: def.success_message.clone(),
        blocking: is_blocking,
        tool_use_id: blocking_tool_use_id.map(ToString::to_string),
        registered_at_ms: now,
        deadline_ms,
        desc: watcher_desc,
        matched_files: matched.matched_files.clone(),
        deferred_panel: DeferredPanel {
            session_key: session_key.clone(),
            display_name: format!("CB: {}", def.name),
            command,
            description: format!("Callback: {}", def.name),
            cwd: def.cwd.clone(),
            callback_id: def.id.clone(),
            callback_name: def.name.clone(),
        },
    };

    let registry = WatcherRegistry::get_mut(state);
    registry.register(Box::new(watcher));

    Ok(session_key)
}

/// Fire all matched non-blocking callbacks.
/// Global: fires once with all files. Local: fires once per matched file.
/// Returns one summary line per invocation in compact format.
pub fn fire_async_callbacks(state: &mut State, callbacks: &[MatchedCallback]) -> Vec<String> {
    let mut summaries = Vec::new();
    for cb in callbacks {
        if cb.definition.is_global {
            match fire_callback(state, cb, None, None) {
                Ok(_) => summaries.push(format!("· {} dispatched", cb.definition.name)),
                Err(e) => summaries.push(format!("· {} FAILED to spawn: {}", cb.definition.name, e)),
            }
        } else {
            for file in &cb.matched_files {
                match fire_callback(state, cb, None, Some(file)) {
                    Ok(_) => summaries.push(format!("· {} dispatched ({})", cb.definition.name, file)),
                    Err(e) => summaries.push(format!("· {} FAILED to spawn for {}: {}", cb.definition.name, file, e)),
                }
            }
        }
    }
    summaries
}

/// Fire all matched blocking callbacks.
/// Global: fires once with all files. Local: fires once per matched file.
/// Each gets a sentinel `tool_use_id` so `tool_pipeline` can track them.
pub fn fire_blocking_callbacks(state: &mut State, callbacks: &[MatchedCallback], tool_use_id: &str) -> Vec<String> {
    let mut summaries = Vec::new();
    for cb in callbacks {
        if cb.definition.is_global {
            match fire_callback(state, cb, Some(tool_use_id), None) {
                Ok(_) => summaries.push(format!("· {} running (blocking)", cb.definition.name)),
                Err(e) => summaries.push(format!("· {} FAILED to spawn: {}", cb.definition.name, e)),
            }
        } else {
            for file in &cb.matched_files {
                match fire_callback(state, cb, Some(tool_use_id), Some(file)) {
                    Ok(_) => summaries.push(format!("· {} running (blocking, {})", cb.definition.name, file)),
                    Err(e) => summaries.push(format!("· {} FAILED to spawn for {}: {}", cb.definition.name, file, e)),
                }
            }
        }
    }
    summaries
}

/// Simple shell escaping: wrap in single quotes, escape any existing single quotes.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

// ============================================================
// CallbackWatcher — fires on process exit with enrichment + auto-close
// ============================================================

/// A watcher that monitors a callback's console session.
///
/// NO panel is created upfront — only on failure/timeout via `create_panel` in `WatcherResult`.
/// On exit 0: returns `success_message` + log file path, kills session.
/// On exit != 0: returns error output + deferred panel info for `tool_cleanup` to create.
#[derive(Debug)]
pub struct CallbackWatcher {
    /// Unique watcher ID (e.g., "`callback_CB3_cb_42`").
    pub watcher_id: String,
    /// Console session key for the spawned script.
    pub session_name: String,
    /// Human-readable callback name.
    pub callback_name: String,
    /// Source tag for watcher registry filtering (e.g., "`callback_CB3`").
    pub callback_tag: &'static str,
    /// Custom message to display on success (exit 0).
    pub success_message: Option<String>,
    /// Whether this watcher blocks the tool pipeline (sentinel replacement).
    pub blocking: bool,
    /// Tool use ID for sentinel matching (blocking watchers only).
    pub tool_use_id: Option<String>,
    /// Timestamp (ms) when this watcher was created.
    pub registered_at_ms: u64,
    /// Timeout deadline (ms since epoch). None = no timeout.
    pub deadline_ms: Option<u64>,
    /// Description shown in the Spine panel's active watchers list.
    pub desc: String,
    /// Files that triggered this callback (for env var injection).
    pub matched_files: Vec<String>,
    /// Panel creation info (deferred until failure/timeout).
    pub deferred_panel: DeferredPanel,
}

impl Watcher for CallbackWatcher {
    fn id(&self) -> &str {
        &self.watcher_id
    }

    fn description(&self) -> &str {
        &self.desc
    }

    fn is_blocking(&self) -> bool {
        self.blocking
    }

    fn tool_use_id(&self) -> Option<&str> {
        self.tool_use_id.as_deref()
    }

    fn check(&self, state: &State) -> Option<WatcherResult> {
        let cs = ConsoleState::get(state);
        let handle = cs.sessions.get(&self.session_name)?;

        if !handle.get_status().is_terminal() {
            return None;
        }

        let exit_code = handle.get_status().exit_code().unwrap_or(-1);

        // Exit code 7 = "nothing to do" — silent success, suppress entirely.
        // Used by callbacks that fire broadly (e.g., pattern "*") but often have nothing to do.
        // Returning None consumes the watcher without producing any visible result.
        if exit_code == 7 {
            return Some(WatcherResult {
                description: String::new(),
                panel_id: None,
                tool_use_id: self.tool_use_id.clone(),
                close_panel: false,
                create_panel: None,
                processed_already: true,
            });
        }

        if exit_code == 0 {
            let msg = self.success_message.as_ref().map_or_else(
                || format!("· {} passed", self.callback_name),
                |sm| format!("· {} passed ({})", self.callback_name, sm),
            );
            Some(WatcherResult {
                description: msg,
                panel_id: None,
                tool_use_id: self.tool_use_id.clone(),
                close_panel: false,
                create_panel: None,
                processed_already: true,
            })
        } else {
            // Panel content is already final — the pipeline waited for process exit before resuming
            let msg = format!("· {} FAILED (exit {})", self.callback_name, exit_code);
            Some(WatcherResult {
                description: msg,
                panel_id: None,
                tool_use_id: self.tool_use_id.clone(),
                close_panel: false,
                create_panel: Some(DeferredPanel {
                    session_key: self.deferred_panel.session_key.clone(),
                    display_name: self.deferred_panel.display_name.clone(),
                    command: self.deferred_panel.command.clone(),
                    description: self.deferred_panel.description.clone(),
                    cwd: self.deferred_panel.cwd.clone(),
                    callback_id: self.deferred_panel.callback_id.clone(),
                    callback_name: self.deferred_panel.callback_name.clone(),
                }),
                processed_already: false,
            })
        }
    }

    fn check_timeout(&self) -> Option<WatcherResult> {
        let deadline = self.deadline_ms?;
        let now = now_ms();
        if now < deadline {
            return None;
        }
        let elapsed_s = ms_to_secs(now.saturating_sub(self.registered_at_ms));
        Some(WatcherResult {
            description: format!("· {} TIMED OUT ({}s)", self.callback_name, elapsed_s,),
            panel_id: None,
            tool_use_id: self.tool_use_id.clone(),
            close_panel: false,
            create_panel: Some(DeferredPanel {
                session_key: self.deferred_panel.session_key.clone(),
                display_name: self.deferred_panel.display_name.clone(),
                command: self.deferred_panel.command.clone(),
                description: self.deferred_panel.description.clone(),
                cwd: self.deferred_panel.cwd.clone(),
                callback_id: self.deferred_panel.callback_id.clone(),
                callback_name: self.deferred_panel.callback_name.clone(),
            }),
            processed_already: false,
        })
    }

    fn registered_ms(&self) -> u64 {
        self.registered_at_ms
    }

    fn source_tag(&self) -> &'static str {
        self.callback_tag
    }

    fn suicide(&self, state: &State) -> bool {
        let cs = ConsoleState::get(state);
        !cs.sessions.contains_key(&self.session_name)
    }

    fn is_easy_bash(&self) -> bool {
        false
    }

    fn is_persistent(&self) -> bool {
        false
    }

    fn fire_at_ms(&self) -> Option<u64> {
        None
    }

    fn message(&self) -> Option<&str> {
        None
    }
}
