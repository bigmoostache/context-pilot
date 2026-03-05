use cp_base::config::INJECTIONS;
use cp_base::panels::now_ms;
use cp_base::state::context::{Kind, make_default_entry};
use cp_base::state::runtime::State;
use cp_base::state::watchers::WatcherRegistry;
use cp_base::tools::{ToolResult, ToolUse};

use crate::manager::SessionHandle;
use crate::types::{ConsoleState, ConsoleWatcher, format_wait_result};

/// Truncate a string to at most `max_bytes` without splitting a UTF-8 char.
fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    s.get(..end).unwrap_or("")
}

/// Sentinel value returned when a blocking `console_wait` is registered.
/// The binary's event loop replaces this with the real result when satisfied.
pub const CONSOLE_WAIT_BLOCKING_SENTINEL: &str = "__CONSOLE_WAIT_BLOCKING__";

/// Maximum execution time for `debug_bash` (blocking tool — must be short).
const BASH_MAX_EXECUTION_SECS: u64 = 10;

/// Check if a command string contains git or gh commands.
/// Returns `Some(error_message)` if blocked, None if allowed.
fn check_git_gh_guardrail(input: &str) -> Option<String> {
    // Split on shell operators to handle chained commands
    let segments: Vec<&str> = input.split(['|', ';', '&', '\n']).collect();

    for segment in &segments {
        let trimmed = segment.trim();
        // Skip empty segments
        if trimmed.is_empty() {
            continue;
        }
        // Strip leading env vars (KEY=VAL) to find the actual command
        let cmd_part = trimmed.split_whitespace().find(|w| !w.contains('=') || w.starts_with('=')).unwrap_or("");

        // Check the actual binary name (could be a path like /usr/bin/git)
        let binary = cmd_part.rsplit('/').next().unwrap_or(cmd_part);

        if binary == "git" {
            return Some(INJECTIONS.console_guardrails.git.trim_end().to_string());
        }
        if binary == "gh" {
            return Some(INJECTIONS.console_guardrails.gh.trim_end().to_string());
        }
        if binary == "typst" {
            return Some(INJECTIONS.console_guardrails.typst.trim_end().to_string());
        }
    }

    None
}

/// Resolve a panel ID (e.g. "P11") to the internal session key.
/// Returns (`session_key`, `panel_id`) or an error.
fn resolve_session_key(state: &State, panel_id: &str) -> Result<String, String> {
    state
        .context
        .iter()
        .find(|c| c.id == panel_id && c.context_type.as_str() == Kind::CONSOLE)
        .and_then(|c| c.get_meta_str("console_name").map(ToString::to_string))
        .ok_or_else(|| format!("Console panel '{panel_id}' not found"))
}

/// Handle `console_create`: spawn a child process and create a panel for its output.
pub fn execute_create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let command = match tool.input.get("command").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return ToolResult::new(tool.id.clone(), "Missing required 'command' parameter".to_string(), true),
    };

    // Guardrail: block git/gh commands
    if let Some(msg) = check_git_gh_guardrail(&command) {
        return ToolResult::new(tool.id.clone(), msg, true);
    }

    let cwd = tool.input.get("cwd").and_then(|v| v.as_str()).map(ToString::to_string);
    let description = tool.input.get("description").and_then(|v| v.as_str()).map(ToString::to_string);

    // Auto-generate session key
    let session_key = {
        let cs = ConsoleState::get_mut(state);
        let key = format!("c_{}", cs.next_session_id);
        cs.next_session_id = cs.next_session_id.saturating_add(1);
        key
    };

    // Spawn the process
    let handle = match SessionHandle::spawn(session_key.clone(), command.clone(), cwd.clone()) {
        Ok(h) => h,
        Err(e) => return ToolResult::new(tool.id.clone(), e, true),
    };

    // Display name: description if provided, else truncated command
    let display_name = description.as_deref().unwrap_or_else(|| truncate_str(&command, 30));

    // Create dynamic panel with UID for persistence
    let panel_id = state.next_available_context_id();
    let uid = format!("UID_{}_P", state.global_next_uid);
    state.global_next_uid = state.global_next_uid.saturating_add(1);
    let mut ctx = make_default_entry(&panel_id, Kind::new(Kind::CONSOLE), display_name, true);
    ctx.uid = Some(uid);
    ctx.set_meta("console_name", &session_key);
    ctx.set_meta("console_command", &command);
    ctx.set_meta("console_status", &handle.get_status().label());
    if let Some(ref desc) = description {
        ctx.set_meta("console_description", desc);
    }
    if let Some(ref dir) = cwd {
        ctx.set_meta("console_cwd", dir);
    }
    state.context.push(ctx);

    // Store handle
    let cs = ConsoleState::get_mut(state);
    drop(cs.sessions.insert(session_key, handle));

    ToolResult::new(tool.id.clone(), format!("Console created in {panel_id}"), false)
}

/// Handle `console_send_keys`: write input text to a running process's stdin.
pub fn execute_send_keys(tool: &ToolUse, state: &mut State) -> ToolResult {
    let panel_id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return ToolResult::new(tool.id.clone(), "Missing required 'id' parameter".to_string(), true),
    };
    let input = match tool.input.get("input").and_then(|v| v.as_str()) {
        Some(i) => i.to_string(),
        None => return ToolResult::new(tool.id.clone(), "Missing required 'input' parameter".to_string(), true),
    };

    // Guardrail: block git/gh commands sent to interactive shells
    if let Some(msg) = check_git_gh_guardrail(&input) {
        return ToolResult::new(tool.id.clone(), msg, true);
    }

    let session_key = match resolve_session_key(state, &panel_id) {
        Ok(k) => k,
        Err(e) => return ToolResult::new(tool.id.clone(), e, true),
    };

    let cs = ConsoleState::get(state);
    let Some(handle) = cs.sessions.get(&session_key) else {
        return ToolResult::new(tool.id.clone(), format!("Session for '{panel_id}' not found"), true);
    };

    if handle.get_status().is_terminal() {
        return ToolResult::new(
            tool.id.clone(),
            format!("Console '{}' has already exited ({})", panel_id, handle.get_status().label()),
            true,
        );
    }

    if let Err(e) = handle.send_input(&input) {
        return ToolResult::new(tool.id.clone(), format!("Failed to send input: {e}"), true);
    }

    // Short delay for output to arrive
    state.tool_sleep_until_ms = now_ms().saturating_add(500);

    ToolResult::new(tool.id.clone(), format!("Sent input to console '{panel_id}'"), false)
}

/// Handle `console_wait`: register a blocking watcher for exit or pattern match.
pub fn execute_wait(tool: &ToolUse, state: &mut State) -> ToolResult {
    let panel_id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return ToolResult::new(tool.id.clone(), "Missing required 'id' parameter".to_string(), true),
    };
    let mode = match tool.input.get("mode").and_then(|v| v.as_str()) {
        Some(m) => m.to_string(),
        None => return ToolResult::new(tool.id.clone(), "Missing required 'mode' parameter".to_string(), true),
    };
    let pattern = tool.input.get("pattern").and_then(|v| v.as_str()).map(ToString::to_string);
    let max_wait: u64 = tool.input.get("max_wait").and_then(serde_json::Value::as_u64).unwrap_or(30).clamp(1, 30);

    // Validate mode
    if mode != "exit" && mode != "pattern" {
        return ToolResult::new(tool.id.clone(), format!("Invalid mode '{mode}'. Must be 'exit' or 'pattern'."), true);
    }

    if mode == "pattern" && pattern.is_none() {
        return ToolResult::new(tool.id.clone(), "Mode 'pattern' requires a 'pattern' parameter".to_string(), true);
    }

    let session_key = match resolve_session_key(state, &panel_id) {
        Ok(k) => k,
        Err(e) => return ToolResult::new(tool.id.clone(), e, true),
    };

    // Check if session exists
    let cs = ConsoleState::get(state);
    let Some(handle) = cs.sessions.get(&session_key) else {
        return ToolResult::new(tool.id.clone(), format!("Session for '{panel_id}' not found"), true);
    };

    // Check if condition is already met
    let already_met = match mode.as_str() {
        "exit" => handle.get_status().is_terminal(),
        "pattern" => pattern.as_ref().is_some_and(|pat| handle.buffer.contains_pattern(pat)),
        _ => false,
    };

    if already_met {
        let exit_code = handle.get_status().exit_code();
        let last_lines = handle.buffer.last_n_lines(5);
        return ToolResult::new(
            tool.id.clone(),
            format_wait_result(&session_key, exit_code, &panel_id, &last_lines),
            false,
        );
    }

    let now = now_ms();
    let desc = match mode.as_str() {
        "exit" => format!("⏳ Waiting for {panel_id} to exit"),
        "pattern" => format!("⏳ Waiting for pattern '{}' in {}", pattern.as_deref().unwrap_or("?"), panel_id),
        _ => format!("⏳ Waiting on {panel_id}"),
    };

    let watcher = ConsoleWatcher {
        watcher_id: format!("console_{session_key}_{mode}"),
        session_name: session_key,
        mode,
        pattern,
        blocking: true,
        tool_use_id: Some(tool.id.clone()),
        registered_at_ms: now,
        deadline_ms: Some(now.saturating_add(max_wait.saturating_mul(1000))),
        easy_bash: false,
        panel_id,
        desc,
    };

    let registry = WatcherRegistry::get_mut(state);
    registry.register(Box::new(watcher));

    ToolResult::new(tool.id.clone(), CONSOLE_WAIT_BLOCKING_SENTINEL.to_string(), false)
}

/// Handle `console_watch`: register an async (non-blocking) watcher with spine notification.
pub fn execute_watch(tool: &ToolUse, state: &mut State) -> ToolResult {
    let panel_id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return ToolResult::new(tool.id.clone(), "Missing required 'id' parameter".to_string(), true),
    };
    let mode = match tool.input.get("mode").and_then(|v| v.as_str()) {
        Some(m) => m.to_string(),
        None => return ToolResult::new(tool.id.clone(), "Missing required 'mode' parameter".to_string(), true),
    };
    let pattern = tool.input.get("pattern").and_then(|v| v.as_str()).map(ToString::to_string);

    // Validate mode
    if mode != "exit" && mode != "pattern" {
        return ToolResult::new(tool.id.clone(), format!("Invalid mode '{mode}'. Must be 'exit' or 'pattern'."), true);
    }

    if mode == "pattern" && pattern.is_none() {
        return ToolResult::new(tool.id.clone(), "Mode 'pattern' requires a 'pattern' parameter".to_string(), true);
    }

    let session_key = match resolve_session_key(state, &panel_id) {
        Ok(k) => k,
        Err(e) => return ToolResult::new(tool.id.clone(), e, true),
    };

    // Check if session exists
    let cs = ConsoleState::get(state);
    let Some(handle) = cs.sessions.get(&session_key) else {
        return ToolResult::new(tool.id.clone(), format!("Session for '{panel_id}' not found"), true);
    };

    // Check if condition is already met — return immediately
    let already_met = match mode.as_str() {
        "exit" => handle.get_status().is_terminal(),
        "pattern" => pattern.as_ref().is_some_and(|pat| handle.buffer.contains_pattern(pat)),
        _ => false,
    };

    if already_met {
        let exit_code = handle.get_status().exit_code();
        let last_lines = handle.buffer.last_n_lines(5);
        return ToolResult::new(
            tool.id.clone(),
            format_wait_result(&session_key, exit_code, &panel_id, &last_lines),
            false,
        );
    }

    let now = now_ms();
    let desc = match mode.as_str() {
        "exit" => format!("👁 Watching {panel_id} for exit"),
        "pattern" => format!("👁 Watching {} for '{}'", panel_id, pattern.as_deref().unwrap_or("?")),
        _ => format!("👁 Watching {panel_id}"),
    };

    let watcher = ConsoleWatcher {
        watcher_id: format!("console_{session_key}_{mode}"),
        session_name: session_key,
        mode,
        pattern,
        blocking: false,
        tool_use_id: None,
        registered_at_ms: now,
        deadline_ms: None, // async watchers have no timeout
        easy_bash: false,
        panel_id: panel_id.clone(),
        desc,
    };

    let registry = WatcherRegistry::get_mut(state);
    registry.register(Box::new(watcher));

    ToolResult::new(
        tool.id.clone(),
        format!("Watcher registered for '{panel_id}' — you'll get a spine notification when the condition is met."),
        false,
    )
}

/// Handle `console_easy_bash`: spawn, block until exit or 10s timeout, return output summary.
pub fn execute_debug_bash(tool: &ToolUse, state: &mut State) -> ToolResult {
    let command = match tool.input.get("command").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return ToolResult::new(tool.id.clone(), "Missing required 'command' parameter".to_string(), true),
    };

    // Guardrail: block git/gh commands
    if let Some(msg) = check_git_gh_guardrail(&command) {
        return ToolResult::new(tool.id.clone(), msg, true);
    }

    let cwd = tool.input.get("cwd").and_then(|v| v.as_str()).map(ToString::to_string);

    // Spawn via the console server (non-blocking to the main loop)
    let session_key = {
        let cs = ConsoleState::get_mut(state);
        let key = format!("c_{}", cs.next_session_id);
        cs.next_session_id = cs.next_session_id.saturating_add(1);
        key
    };

    let handle = match SessionHandle::spawn(session_key.clone(), command.clone(), cwd.clone()) {
        Ok(h) => h,
        Err(e) => return ToolResult::new(tool.id.clone(), format!("Failed to execute: {e}"), true),
    };

    // Create a panel so output goes there instead of flooding the conversation
    let display_name = truncate_str(&command, 30);
    let panel_id = state.next_available_context_id();
    let uid = format!("UID_{}_P", state.global_next_uid);
    state.global_next_uid = state.global_next_uid.saturating_add(1);
    let mut ctx = make_default_entry(&panel_id, Kind::new(Kind::CONSOLE), display_name, true);
    ctx.uid = Some(uid);
    ctx.set_meta("console_name", &session_key);
    ctx.set_meta("console_command", &command);
    ctx.set_meta("console_status", &handle.get_status().label());
    ctx.set_meta("console_is_easy_bash", &"true".to_string());
    if let Some(ref dir) = cwd {
        ctx.set_meta("console_cwd", dir);
    }
    state.context.push(ctx);

    // Store the handle (needed for waiter to check status + read buffer)
    let cs = ConsoleState::get_mut(state);
    drop(cs.sessions.insert(session_key.clone(), handle));

    // Register a blocking exit watcher via WatcherRegistry
    let now = now_ms();
    let watcher = ConsoleWatcher {
        watcher_id: format!("console_{session_key}_easy_bash"),
        session_name: session_key,
        mode: "exit".to_string(),
        pattern: None,
        blocking: true,
        tool_use_id: Some(tool.id.clone()),
        registered_at_ms: now,
        deadline_ms: Some(now.saturating_add(BASH_MAX_EXECUTION_SECS.saturating_mul(1000))),
        easy_bash: true,
        panel_id,
        desc: format!("⏳ easy_bash: {}", truncate_str(&command, 40)),
    };

    let registry = WatcherRegistry::get_mut(state);
    registry.register(Box::new(watcher));

    ToolResult::new(tool.id.clone(), CONSOLE_WAIT_BLOCKING_SENTINEL.to_string(), false)
}
