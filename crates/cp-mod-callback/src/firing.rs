//! Callback firing logic: spawn scripts, register watchers.
//!
//! Separated from trigger.rs which handles file collection and pattern matching.

// Queue ID test marker — delete me later
use cp_base::config::constants;
use cp_base::panels::now_ms;
use cp_base::panels::time_arith::ms_to_secs;
use cp_base::state::runtime::State;
use cp_base::state::watchers::carriers::{DeferredPanel, WatcherResult};
use cp_base::state::watchers::{Watcher, WatcherRegistry};

use cp_mod_console::manager::SessionHandle;
use cp_mod_console::types::ConsoleState;

use crate::trigger::{MatchedCallback, build_changed_files_env};
use crate::types::CallbackState;

/// Result of firing a callback, including dedup info.
#[derive(Debug)]
pub struct FireResult {
    /// Console session key for the spawned script.
    pub session_key: String,
    /// Whether an existing session for the same callback was killed and replaced.
    pub replaced: bool,
}

/// Kill an existing session for the same callback definition (dedup).
///
/// If the same callback already has an active session, kills its process,
/// removes its watcher, and cleans up the console entry. Returns `true`
/// if a running session was replaced.
fn kill_existing_callback(state: &mut State, callback_id: &str) -> bool {
    let cs = CallbackState::get_mut(state);
    let Some(old_key) = cs.active_sessions.remove(callback_id) else {
        return false;
    };

    // Kill the old process if still alive
    let console = ConsoleState::get(state);
    if let Some(handle) = console.sessions.get(&old_key)
        && !handle.get_status().is_terminal()
    {
        handle.kill();
    }

    // Remove session from console state
    drop(ConsoleState::get_mut(state).sessions.remove(&old_key));

    // Remove watcher from registry
    let tag = format!("callback_{callback_id}");
    WatcherRegistry::get_mut(state).remove_by_tag(&tag);

    true
}

/// Build the shell command for a callback: either its inline `built_in_command`
/// or a `bash <script>` invocation, with the changed-files env, project root,
/// and callback name baked in as leading `KEY=VAL` assignments.
///
/// # Errors
///
/// Returns `Err` when a non-built-in callback's script file is missing.
fn build_callback_command(
    def: &crate::types::CallbackDefinition,
    env_key: &str,
    env_val: &str,
    project_root: &str,
) -> Result<String, String> {
    if def.built_in {
        let base_cmd = def.built_in_command.as_deref().unwrap_or("echo 'no built_in_command set'");
        return Ok(format!(
            "{env_key}={changed} CP_PROJECT_ROOT={root} CP_CALLBACK_NAME={name} {cmd}",
            changed = shell_escape(env_val),
            root = shell_escape(project_root),
            name = shell_escape(&def.name),
            cmd = base_cmd,
        ));
    }

    let scripts_dir = std::path::PathBuf::from(constants::STORE_DIR).join("scripts");
    let script_path = scripts_dir.join(format!("{}.sh", def.name));
    let script_path_str = if script_path.is_absolute() {
        script_path.to_string_lossy().to_string()
    } else {
        format!("{}/{}", project_root, script_path.to_string_lossy())
    };

    if !script_path.exists() {
        return Err(format!("Callback '{}' script not found: {}", def.name, script_path.display()));
    }

    Ok(format!(
        "{env_key}={changed} CP_PROJECT_ROOT={root} CP_CALLBACK_NAME={name} bash {script}",
        changed = shell_escape(env_val),
        root = shell_escape(project_root),
        name = shell_escape(&def.name),
        script = shell_escape(&script_path_str),
    ))
}

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
) -> Result<FireResult, String> {
    let _fg = cp_base::flame!("cb_fire");
    let def = &matched.definition;

    // Build the command with env vars baked in
    // Global: CP_CHANGED_FILES (plural, all matched files)
    // Local:  CP_CHANGED_FILE  (singular, one file per invocation)
    let (env_key, env_val) = single_file.map_or_else(
        || ("CP_CHANGED_FILES", build_changed_files_env(&matched.matched_files)),
        |file| ("CP_CHANGED_FILE", file.to_owned()),
    );
    let project_root = std::env::current_dir().unwrap_or_default().to_string_lossy().to_string();

    // Use the callback's cwd if set, otherwise project root
    let cwd = def.cwd.clone().or_else(|| Some(project_root.clone()));

    let command = build_callback_command(def, env_key, &env_val, &project_root)?;

    // Dedup: kill any existing session for the same callback definition
    let replaced = kill_existing_callback(state, &def.id);

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
        tool_use_id: blocking_tool_use_id.map(str::to_owned),
        registered_at_ms: now,
        deadline_ms,
        desc: watcher_desc,
        matched_files: matched.matched_files.clone(),
        deferred_panel: DeferredPanel::new(
            session_key.clone(),
            format!("CB: {}", def.name),
            command,
            format!("Callback: {}", def.name),
        )
        .cwd(def.cwd.clone())
        .callback(def.id.clone(), def.name.clone()),
    };

    let registry = WatcherRegistry::get_mut(state);
    registry.register(Box::new(watcher));

    // Track this session for dedup
    drop(CallbackState::get_mut(state).active_sessions.insert(def.id.clone(), session_key.clone()));

    Ok(FireResult { session_key, replaced })
}

/// Dispatch parameters distinguishing async vs blocking callback fan-out.
struct FanOut<'fan> {
    /// Sentinel tool-use id for blocking callbacks (`None` for async).
    blocking_id: Option<&'fan str>,
    /// Verb shown on success ("dispatched" vs "running (blocking)").
    running_word: &'fan str,
    /// Suffix labeling a dedup replacement (async vs blocking wording).
    replaced_suffix: &'fan str,
}

/// Fire one matched callback, fanning out to one invocation per file for
/// local callbacks (or a single all-files invocation for global ones), and
/// push a compact summary line per invocation into `summaries`.
fn fire_one(state: &mut State, cb: &MatchedCallback, fan: &FanOut<'_>, summaries: &mut Vec<String>) {
    let name = &cb.definition.name;
    let files: Vec<Option<String>> =
        if cb.definition.is_global { vec![None] } else { cb.matched_files.iter().map(|f| Some(f.clone())).collect() };

    for file in files {
        let scope = file.as_ref().map(|f| format!(" ({f})")).unwrap_or_default();
        match fire_callback(state, cb, fan.blocking_id, file.as_deref()) {
            Ok(r) => {
                let suffix = if r.replaced { fan.replaced_suffix } else { "" };
                summaries.push(format!("· {name} {}{scope}{suffix}", fan.running_word));
            }
            Err(e) => summaries.push(format!("· {name} FAILED to spawn{scope}: {e}")),
        }
    }
}

/// Fire all matched non-blocking callbacks.
/// Global: fires once with all files. Local: fires once per matched file.
/// Returns one summary line per invocation in compact format.
pub fn fire_async_callbacks(state: &mut State, callbacks: &[MatchedCallback]) -> Vec<String> {
    let _fg = cp_base::flame!("cb_fire_async");
    let fan = FanOut { blocking_id: None, running_word: "dispatched", replaced_suffix: " (replaced previous run)" };
    let mut summaries = Vec::new();
    for cb in callbacks {
        fire_one(state, cb, &fan, &mut summaries);
    }
    summaries
}

/// Fire all matched blocking callbacks.
/// Global: fires once with all files. Local: fires once per matched file.
/// Each gets a sentinel `tool_use_id` so `tool_pipeline` can track them.
pub fn fire_blocking_callbacks(state: &mut State, callbacks: &[MatchedCallback], tool_use_id: &str) -> Vec<String> {
    let _fg = cp_base::flame!("cb_fire_blocking");
    let fan = FanOut {
        blocking_id: Some(tool_use_id),
        running_word: "running (blocking)",
        replaced_suffix: " \u{2014} replaced timed-out run",
    };
    let mut summaries = Vec::new();
    for cb in callbacks {
        fire_one(state, cb, &fan, &mut summaries);
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

        let exit_code = handle.get_status().exit_code().unwrap_or(-1i32);

        // Exit code 7 = "nothing to do" — silent success, suppress entirely.
        // Used by callbacks that fire broadly (e.g., pattern "*") but often have nothing to do.
        // Returning None consumes the watcher without producing any visible result.
        if exit_code == 7i32 {
            return Some(
                WatcherResult::new(String::new())
                    .tool_use_id_opt(self.tool_use_id.clone())
                    .processed_already()
                    .kill_session(self.session_name.clone()),
            );
        }

        if exit_code == 0 {
            let msg = self.success_message.as_ref().map_or_else(
                || format!("· {} passed", self.callback_name),
                |sm| format!("· {} passed ({})", self.callback_name, sm),
            );
            Some(
                WatcherResult::new(msg)
                    .tool_use_id_opt(self.tool_use_id.clone())
                    .processed_already()
                    .kill_session(self.session_name.clone()),
            )
        } else {
            // Panel content is already final — the pipeline waited for process exit before resuming
            let msg = format!("· {} FAILED (exit {})", self.callback_name, exit_code);
            Some(
                WatcherResult::new(msg).tool_use_id_opt(self.tool_use_id.clone()).create_panel(
                    DeferredPanel::new(
                        self.deferred_panel.session_key.clone(),
                        self.deferred_panel.display_name.clone(),
                        self.deferred_panel.command.clone(),
                        self.deferred_panel.description.clone(),
                    )
                    .cwd(self.deferred_panel.cwd.clone())
                    .callback(self.deferred_panel.callback_id.clone(), self.deferred_panel.callback_name.clone()),
                ),
            )
        }
    }

    fn check_timeout(&self) -> Option<WatcherResult> {
        let deadline = self.deadline_ms?;
        let now = now_ms();
        if now < deadline {
            return None;
        }
        let elapsed_s = ms_to_secs(now.saturating_sub(self.registered_at_ms));
        Some(
            WatcherResult::new(format!("· {} TIMED OUT ({}s)", self.callback_name, elapsed_s))
                .tool_use_id_opt(self.tool_use_id.clone())
                .create_panel(
                    DeferredPanel::new(
                        self.deferred_panel.session_key.clone(),
                        self.deferred_panel.display_name.clone(),
                        self.deferred_panel.command.clone(),
                        self.deferred_panel.description.clone(),
                    )
                    .cwd(self.deferred_panel.cwd.clone())
                    .callback(self.deferred_panel.callback_id.clone(), self.deferred_panel.callback_name.clone()),
                ),
        )
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

    fn thread_id(&self) -> Option<&str> {
        None
    }

    fn interval_ms(&self) -> u64 {
        0
    }

    fn recurrence_label(&self) -> Option<&str> {
        None
    }
}
