//! Callback firing logic: spawn scripts, register watchers.
//!
//! Separated from trigger.rs which handles file collection and pattern matching.

use cp_base::config::constants::STORE_DIR;
use cp_base::panels::now_ms;
use cp_base::state::State;
use cp_base::watchers::{DeferredPanel, Watcher, WatcherRegistry, WatcherResult};

use cp_mod_console::manager::SessionHandle;
use cp_mod_console::types::ConsoleState;

use crate::trigger::{MatchedCallback, build_changed_files_env};

/// Fire a single callback by spawning its script via the console server.
/// Creates a console session + watcher (no panel — deferred until failure).
///
/// # Errors
///
/// Returns `Err(message)` if the script fails to execute or times out.
pub fn fire_callback(
    state: &mut State,
    matched: &MatchedCallback,
    blocking_tool_use_id: Option<&str>,
) -> Result<String, String> {
    let def = &matched.definition;

    // one_at_a_time: skip if this callback already has a running watcher
    if def.one_at_a_time {
        let tag = format!("callback_{}", def.id);
        let registry = WatcherRegistry::get(state);
        if registry.has_watcher_with_tag(&tag) {
            return Err(format!("Callback '{}' skipped (one_at_a_time: already running)", def.name,));
        }
    }

    // Build the command with env vars baked in
    let changed_files_env = build_changed_files_env(&matched.matched_files);
    let project_root = std::env::current_dir().unwrap_or_default().to_string_lossy().to_string();

    // Use the callback's cwd if set, otherwise project root
    let cwd = def.cwd.clone().or_else(|| Some(project_root.clone()));

    // Build the script path — uses STORE_DIR for scripts dir
    // For built-in callbacks, use the built_in_command directly instead of a script file.
    let command = if def.built_in {
        let base_cmd = def.built_in_command.as_deref().unwrap_or("echo 'no built_in_command set'");
        format!(
            "CP_CHANGED_FILES={changed_files} CP_PROJECT_ROOT={root} CP_CALLBACK_NAME={name} {cmd}",
            changed_files = shell_escape(&changed_files_env),
            root = shell_escape(&project_root),
            name = shell_escape(&def.name),
            cmd = base_cmd,
        )
    } else {
        let scripts_dir = std::path::PathBuf::from(STORE_DIR).join("scripts");
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
            "CP_CHANGED_FILES={changed_files} CP_PROJECT_ROOT={root} CP_CALLBACK_NAME={name} bash {script}",
            changed_files = shell_escape(&changed_files_env),
            root = shell_escape(&project_root),
            name = shell_escape(&def.name),
            script = shell_escape(&script_path_str),
        )
    };

    // Generate session key via console state
    let session_key = {
        let cs = ConsoleState::get_mut(state);
        let key = format!("cb_{}", cs.next_session_id);
        cs.next_session_id += 1;
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
    let deadline_ms = def.timeout_secs.map(|t| now + t * 1000);

    let watcher_desc = if is_blocking {
        format!("⏳ Callback '{}' (blocking)", def.name)
    } else {
        format!("👁 Callback '{}'", def.name)
    };

    let watcher = CallbackWatcher {
        watcher_id: format!("callback_{}_{}", def.id, session_key),
        session_name: session_key.clone(),
        callback_name: def.name.clone(),
        callback_tag: format!("callback_{}", def.id),
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
/// Returns one summary line per callback in compact format: "· name dispatched"
pub fn fire_async_callbacks(state: &mut State, callbacks: &[MatchedCallback]) -> Vec<String> {
    let mut summaries = Vec::new();
    for cb in callbacks {
        match fire_callback(state, cb, None) {
            Ok(_session_key) => {
                summaries.push(format!("· {} dispatched", cb.definition.name));
            }
            Err(e) => {
                summaries.push(format!("· {} FAILED to spawn: {}", cb.definition.name, e));
            }
        }
    }
    summaries
}

/// Fire all matched blocking callbacks.
/// Each gets a sentinel `tool_use_id` so `tool_pipeline` can track them.
/// Returns one summary line per callback: "· name running (blocking)"
pub fn fire_blocking_callbacks(state: &mut State, callbacks: &[MatchedCallback], tool_use_id: &str) -> Vec<String> {
    let mut summaries = Vec::new();
    for cb in callbacks {
        match fire_callback(state, cb, Some(tool_use_id)) {
            Ok(_session_key) => {
                summaries.push(format!("· {} running (blocking)", cb.definition.name));
            }
            Err(e) => {
                summaries.push(format!("· {} FAILED to spawn: {}", cb.definition.name, e));
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
    pub callback_tag: String,
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
            let log_path = cp_mod_console::manager::log_file_path(&self.session_name);
            let log_path_str = log_path.to_string_lossy();
            #[expect(clippy::option_if_let_else, reason = "if-let is clearer here")]
            let msg = if let Some(ref sm) = self.success_message {
                format!("· {} passed ({}). Log: {}", self.callback_name, sm, log_path_str)
            } else {
                format!("· {} passed. Log: {}", self.callback_name, log_path_str)
            };
            Some(WatcherResult {
                description: msg,
                panel_id: None,
                tool_use_id: self.tool_use_id.clone(),
                close_panel: false,
                create_panel: None,
                processed_already: true,
            })
        } else {
            let last_lines = handle.buffer.last_n_lines(3);
            let msg = format!(
                "· {} FAILED (exit {})\n{}",
                self.callback_name,
                exit_code,
                last_lines.lines().map(|l| format!("    {l}")).collect::<Vec<_>>().join("\n"),
            );
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
        let elapsed_s = (now - self.registered_at_ms) / 1000;
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

    fn source_tag(&self) -> &str {
        &self.callback_tag
    }

    fn suicide(&self, state: &State) -> bool {
        let cs = ConsoleState::get(state);
        !cs.sessions.contains_key(&self.session_name)
    }
}
