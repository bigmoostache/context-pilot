//! Console module — spawn and manage child processes via a background Unix socket server.
//!
//! Provides 5 tools: `console_create`, `console_send_keys`, `console_wait`,
//! `console_watch`, and `console_easy_bash`. Each session gets a panel showing
//! its ring-buffered output, and survives TUI reloads via server reconnection.

/// Session management: spawn/reconnect via Unix socket server, kill, orphan cleanup.
pub mod manager;
/// Panel rendering for console session output.
mod panel;
/// Background polling threads for log tailing and process status.
mod pollers;
/// Thread-safe ring buffer for capturing process output.
pub mod ring_buffer;
/// Tool implementations: create, `send_keys`, wait, watch, `easy_bash`.
pub mod tools;
/// Console state types: `ConsoleState`, `SessionMeta`, `ProcessStatus`, `ConsoleWatcher`.
pub mod types;

/// Subdirectory under `STORE_DIR` for console log files.
pub const CONSOLE_DIR: &str = "console";

use std::collections::HashMap;
use std::io::Write as _;

use serde_json::json;

use cp_base::modules::{Module, ToolVisualizer};
use cp_base::panels::Panel;
use cp_base::state::context::{Kind, TypeMeta};
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ParamType, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::manager::SessionHandle;
use self::panel::ConsolePanel;
use self::types::{ConsoleState, SessionMeta};

use cp_base::cast::Safe as _;

/// Lazily parsed tool descriptions from the console YAML definition.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/console.yaml")));

/// Console module: spawns child processes, manages sessions, provides interactive I/O.
#[derive(Debug, Clone, Copy)]
pub struct ConsoleModule;

impl Module for ConsoleModule {
    fn id(&self) -> &'static str {
        "console"
    }
    fn name(&self) -> &'static str {
        "Console"
    }
    fn description(&self) -> &'static str {
        "Spawn and manage child processes"
    }

    fn init_state(&self, state: &mut State) {
        state.set_ext(ConsoleState::new());
        // Ensure the console server is running
        if let Err(e) = manager::find_or_create_server() {
            drop(writeln!(std::io::stderr(), "Console server startup failed: {e}"));
        }
    }

    fn reset_state(&self, state: &mut State) {
        // Collect file paths before shutdown (shutdown drains sessions)
        let paths: Vec<String> = {
            let cs = ConsoleState::get(state);
            cs.sessions.values().map(|h| h.log_path.clone()).collect()
        };
        ConsoleState::shutdown_all(state);
        state.set_ext(ConsoleState::new());
        // Clean up log files
        for log in paths {
            let _: Option<()> = std::fs::remove_file(&log).ok();
        }
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let cs = ConsoleState::get(state);
        let mut sessions_map: HashMap<String, SessionMeta> = HashMap::new();
        let mut session_keys: Vec<_> = cs.sessions.keys().cloned().collect();
        session_keys.sort();
        for name in &session_keys {
            let Some(handle) = cs.sessions.get(name) else { continue };
            // Only persist live (non-terminal) sessions
            if !handle.get_status().is_terminal()
                && let Some(pid) = handle.pid()
            {
                // Leak stdin so script doesn't see EOF when TUI exits for reload.
                // This keeps the pipe fd open (no EOF → script stays alive).
                // After reload, send_keys already fails with "stdin unavailable".
                SessionHandle::leak_stdin();

                drop(sessions_map.insert(
                    name.clone(),
                    SessionMeta {
                        pid,
                        command: handle.command.clone(),
                        cwd: handle.cwd.clone(),
                        log_path: handle.log_path.clone(),
                        started_at: handle.started_at,
                    },
                ));
            }
        }
        if sessions_map.is_empty() && cs.next_session_id == 1 {
            serde_json::Value::Null
        } else {
            json!({
                "sessions": sessions_map,
                "next_session_id": cs.next_session_id,
            })
        }
    }
    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        // Server already started in init_state — no need to call find_or_create_server again.

        // Restore counter
        if let Some(v) = data.get("next_session_id").and_then(serde_json::Value::as_u64) {
            let cs = ConsoleState::get_mut(state);
            cs.next_session_id = v.to_usize();
        }

        let sessions_map: HashMap<String, SessionMeta> = match data.get("sessions") {
            Some(v) => match serde_json::from_value(v.clone()) {
                Ok(m) => m,
                Err(_) => return,
            },
            None => return,
        };

        if sessions_map.is_empty() {
            // No known sessions — kill any orphans on the server
            manager::kill_orphaned_processes(&std::collections::HashSet::new());
            return;
        }

        // Collect known session keys for orphan cleanup
        let known_keys: std::collections::HashSet<String> = sessions_map.keys().cloned().collect();

        // Kill any server-managed sessions that aren't in our saved state
        manager::kill_orphaned_processes(&known_keys);

        // Phase 1: Reconnect sessions (no &mut State needed)
        let mut reconnected: Vec<(String, SessionHandle)> = Vec::new();
        let mut sorted_session_names: Vec<_> = sessions_map.keys().cloned().collect();
        sorted_session_names.sort();
        for name in &sorted_session_names {
            let Some(meta) = sessions_map.get(name) else { continue };
            let handle = SessionHandle::reconnect(manager::ReconnectMeta {
                name: name.clone(),
                command: meta.command.clone(),
                cwd: meta.cwd.clone(),
                pid: meta.pid,
                log_path_str: meta.log_path.clone(),
                started_at: meta.started_at,
            });
            reconnected.push((name.clone(), handle));
        }

        // Phase 2: Insert handles into ConsoleState and update panel metadata
        for (name, handle) in reconnected {
            let status_label = handle.get_status().label();
            let cs = ConsoleState::get_mut(state);
            drop(cs.sessions.insert(name.clone(), handle));

            // Update panel metadata if panel was persisted
            if let Some(ctx) = state.context.iter_mut().find(|c| c.get_meta_str("console_name") == Some(&name)) {
                ctx.set_meta("console_status", &status_label);
                ctx.cache_deprecated = true;
            }
        }

        // Phase 3: Remove orphaned console panels that have no matching session
        // (e.g. sessions that were terminal at save time and weren't persisted)
        let live_names: std::collections::HashSet<String> = {
            let cs = ConsoleState::get(state);
            cs.sessions.keys().cloned().collect()
        };
        state.context.retain(|c| {
            if c.context_type.as_str() != Kind::CONSOLE {
                return true; // keep non-console panels
            }
            c.get_meta_str("console_name").is_some_and(|name| live_names.contains(name))
        });
    }

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(Kind::CONSOLE)]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            Kind::CONSOLE => Some(Box::new(ConsolePanel)),
            _ => None,
        }
    }

    fn context_type_metadata(&self) -> Vec<TypeMeta> {
        vec![TypeMeta {
            context_type: "console",
            icon_id: "tmux", // Reuse tmux icon for now
            is_fixed: false,
            needs_cache: true,
            fixed_order: None,
            display_name: "console",
            short_name: "console",
            needs_async_wait: true,
        }]
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("console_create", t)
                .short_desc("Spawn a new process")
                .category("Console")
                .param("command", ParamType::String, true)
                .param("cwd", ParamType::String, false)
                .param("description", ParamType::String, false)
                .build(),
            ToolDefinition::from_yaml("console_send_keys", t)
                .short_desc("Send input to process")
                .category("Console")
                .param("id", ParamType::String, true)
                .param("input", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("console_wait", t)
                .short_desc("Wait for process event")
                .category("Console")
                .param("id", ParamType::String, true)
                .param_enum("mode", &["exit", "pattern"], true)
                .param("pattern", ParamType::String, false)
                .param_with_default("max_wait", ParamType::Integer, "30")
                .build(),
            ToolDefinition::from_yaml("console_watch", t)
                .short_desc("Async watch for process event")
                .category("Console")
                .param("id", ParamType::String, true)
                .param_enum("mode", &["exit", "pattern"], true)
                .param("pattern", ParamType::String, false)
                .build(),
            ToolDefinition::from_yaml("console_easy_bash", t)
                .short_desc("Run a command and return output")
                .category("Console")
                .param("command", ParamType::String, true)
                .param("cwd", ParamType::String, false)
                .build(),
        ]
    }

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<Verdict> {
        match tool.name.as_str() {
            "console_send_keys" | "console_wait" | "console_watch" => {
                let mut pf = Verdict::new();
                if let Some(panel_id) = tool.input.get("id").and_then(|v| v.as_str()) {
                    match state.context.iter().find(|c| c.id == panel_id) {
                        None => pf.errors.push(format!("Panel '{panel_id}' not found")),
                        Some(ctx) if ctx.context_type.as_str() != Kind::CONSOLE => {
                            pf.errors.push(format!("Panel '{panel_id}' is not a console panel"));
                        }
                        Some(ctx) => {
                            // Check if process has exited
                            if let Some(name) = ctx.get_meta_str("console_name")
                                && let Some(handle) = ConsoleState::get(state).sessions.get(name)
                                && handle.get_status().is_terminal()
                            {
                                let status = handle.get_status().label();
                                pf.warnings
                                    .push(format!("Console '{name}' process has {status} — commands may not work"));
                            }
                        }
                    }
                }
                Some(pf)
            }
            _ => None,
        }
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "console_create" => Some(tools::execute_create(tool, state)),
            "console_send_keys" => Some(tools::execute_send_keys(tool, state)),
            "console_wait" => Some(tools::execute_wait(tool, state)),
            "console_watch" => Some(tools::execute_watch(tool, state)),
            "console_easy_bash" => Some(tools::execute_debug_bash(tool, state)),
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![
            ("console_create", visualize_console_output),
            ("console_send_keys", visualize_console_output),
            ("console_wait", visualize_console_output),
            ("console_watch", visualize_console_output),
        ]
    }

    fn on_close_context(
        &self,
        ctx: &cp_base::state::context::Entry,
        state: &mut State,
    ) -> Option<Result<String, String>> {
        if ctx.context_type.as_str() != Kind::CONSOLE {
            return None;
        }
        let name = ctx.get_meta_str("console_name").unwrap_or_default().to_string();
        // Grab file path before removing
        let log_path = {
            let cs = ConsoleState::get(state);
            cs.sessions.get(&name).map(|h| h.log_path.clone()).unwrap_or_default()
        };
        ConsoleState::kill_session(state, &name);
        {
            let cs = ConsoleState::get_mut(state);
            drop(cs.sessions.remove(&name));
        }
        // Delete log file
        if !log_path.is_empty() {
            let _: Option<()> = std::fs::remove_file(&log_path).ok();
        }
        Some(Ok(format!("console: {name}")))
    }

    fn context_detail(&self, ctx: &cp_base::state::context::Entry) -> Option<String> {
        (ctx.context_type.as_str() == Kind::CONSOLE).then(|| {
            let desc =
                ctx.get_meta_str("console_description").or_else(|| ctx.get_meta_str("console_command")).unwrap_or("?");
            let status = ctx.get_meta_str("console_status").unwrap_or("?");
            format!("{desc} ({status})")
        })
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Console", "Spawn and manage child processes")]
    }

    fn dependencies(&self) -> &[&'static str] {
        &[]
    }

    fn is_core(&self) -> bool {
        false
    }

    fn is_global(&self) -> bool {
        false
    }

    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![]
    }

    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
        None
    }

    fn overview_context_section(&self, _state: &State) -> Option<String> {
        None
    }

    fn overview_render_sections(&self, _state: &State) -> Vec<(u8, Vec<cp_render::Block>)> {
        vec![]
    }

    fn on_user_message(&self, _state: &mut State) {}

    fn on_stream_stop(&self, _state: &mut State) {}

    fn on_tool_progress(&self, _tool_name: &str, _input_so_far: &str, _state: &mut State) {}

    fn on_tool_complete(&self, _tool_name: &str, _state: &mut State) {}

    fn watch_paths(&self, _state: &State) -> Vec<cp_base::panels::WatchSpec> {
        vec![]
    }

    fn should_invalidate_on_fs_change(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _changed_path: &str,
        _is_dir_event: bool,
    ) -> bool {
        false
    }

    fn watcher_immediate_refresh(&self) -> bool {
        true
    }
}

/// Visualizer for console tool results — color-codes success/error/info lines.
fn visualize_console_output(content: &str, width: usize) -> Vec<cp_render::Block> {
    use cp_render::{Block, Semantic, Span};

    content
        .lines()
        .map(|line| {
            if line.is_empty() {
                return Block::empty();
            }
            let semantic = if line.starts_with("Error:") || line.starts_with("Failed") || line.starts_with("Missing") {
                Semantic::Error
            } else if line.starts_with("Console ")
                || line.starts_with("Sent ")
                || line.starts_with("Watcher ")
                || line.contains("created")
            {
                Semantic::Success
            } else if line.contains("condition met") || line.contains("Last output:") {
                Semantic::Info
            } else {
                Semantic::Default
            };
            let display = if line.len() > width {
                format!("{}...", line.get(..line.floor_char_boundary(width.saturating_sub(3))).unwrap_or(""))
            } else {
                line.to_string()
            };
            Block::Line(vec![Span::styled(display, semantic)])
        })
        .collect()
}
