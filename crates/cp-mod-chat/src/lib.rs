// Raise recursion limit for matrix-sdk 0.16 async layout computation.
// Upstream: https://github.com/matrix-org/matrix-rust-sdk/issues/6254
#![recursion_limit = "256"]

//! Chat module — Matrix-based universal messaging layer.
//!
//! Provides 7 tools (`Chat_open`, `Chat_send`, `Chat_react`, `Chat_configure`,
//! `Chat_search`, `Chat_create_room`, `Chat_invite`) and
//! 2 panel types (`ChatDashboardPanel`, `ChatRoomPanel`) backed by a local
//! Matrix homeserver (Tuwunel) with transparent bridge support.

/// First-run bootstrap: directory layout, config generation, credential scaffolding.
mod bootstrap;
/// Bridge configuration templates and registration file management.
mod bridges;
/// Matrix SDK client wrapper: connection, authentication, sync loop, sending, event bridge, download.
mod client;
/// Panel rendering: room panels and dashboard.
mod panels;
/// Tuwunel homeserver process lifecycle: start, stop, health check.
mod server;
/// Tool execution handlers for all `Chat_*` tools.
mod tools;
/// Chat state types: `ChatState`, `RoomInfo`, `MessageInfo`, `BridgeSource`, etc.
pub mod types;

use types::ChatState;

// Suppress unused-crate-dependencies for transitive deps pulled in by matrix-sdk.
use url as _;

use std::fmt::Write as _;

use serde_json::json;

use cp_base::modules::{Module, ToolVisualizer};
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ParamType, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::panels::dashboard::ChatDashboardPanel;
use self::panels::room::ChatRoomPanel;

/// Lazily parsed tool descriptions from the chat YAML definition.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/chat.yaml")));

/// Chat module: Matrix-based universal messaging layer.
#[derive(Debug, Clone, Copy)]
pub struct ChatModule;

impl Module for ChatModule {
    fn id(&self) -> &'static str {
        "chat"
    }

    fn name(&self) -> &'static str {
        "Chat"
    }

    fn description(&self) -> &'static str {
        "Matrix-based universal messaging (Telegram, Discord, Slack, Google Chat)"
    }

    fn is_global(&self) -> bool {
        true
    }

    fn is_core(&self) -> bool {
        false
    }

    fn dependencies(&self) -> &[&'static str] {
        &["spine"]
    }

    fn init_state(&self, state: &mut State) {
        state.set_ext(ChatState::default());

        // Run first-time bootstrap (idempotent — skips if files exist)
        if let Err(e) = bootstrap::bootstrap() {
            let cs = ChatState::get_mut(state);
            cs.server_status = types::ServerStatus::Error(format!("Bootstrap failed: {e}"));
            return;
        }

        // Start the homeserver, then connect the Matrix client
        if let Err(e) = server::start_server(state) {
            log::warn!("Chat server failed to start: {e}");
            return;
        }

        // Register this project's bot user, create its room, set display name
        if let Err(e) = bootstrap::project_post_start(state) {
            drop(std::fs::write(".context-pilot/matrix/init_debug.log", format!("project_post_start FAILED: {e}\n")));
            log::warn!("Project post-start setup failed: {e}");
            // Non-fatal — client can still connect with existing credentials
        } else {
            drop(std::fs::write(".context-pilot/matrix/init_debug.log", "project_post_start OK\n"));
        }

        if let Err(e) = client::connect() {
            log::warn!("Matrix client connection failed: {e}");
            return;
        }

        client::start_sync();

        // Recover any running bridge processes from a previous session
        recover_bridges(state);
    }

    fn reset_state(&self, state: &mut State) {
        // Only disconnect the Matrix SDK client and clear local state.
        // The server and bridges are global, shared resources — they
        // survive reloads and are reused by init_state() on restart.
        // Explicit stop_server() is reserved for full module deactivation
        // via module_toggle, which calls reset_state() + drop.
        shutdown_bridges(state);
        client::disconnect();
        state.set_ext(ChatState::default());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let cs = ChatState::get(state);
        json!({
            "search_query": cs.search_query,
            "server_pid": cs.server_pid,
            "bot_user_id": cs.bot_user_id,
        })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        let cs = ChatState::get_mut(state);
        if let Some(q) = data.get("search_query").and_then(serde_json::Value::as_str) {
            cs.search_query = Some(q.to_string());
        }
        if let Some(pid) = data.get("server_pid").and_then(serde_json::Value::as_u64) {
            cs.server_pid = u32::try_from(pid).ok();
        }
        if let Some(uid) = data.get("bot_user_id").and_then(serde_json::Value::as_str) {
            cs.bot_user_id = Some(uid.to_string());
        }
    }

    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(Kind::CHAT_DASHBOARD)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![(Kind::new(Kind::CHAT_DASHBOARD), "Chat", false)]
    }

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new("chat:room")]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        let ct = context_type.as_str();
        if ct == Kind::CHAT_DASHBOARD {
            Some(Box::new(ChatDashboardPanel))
        } else if ct.starts_with("chat:") {
            Some(Box::new(ChatRoomPanel))
        } else {
            None
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("Chat_open", t)
                .short_desc("Open a room panel")
                .category("Chat")
                .param("room", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("Chat_send", t)
                .short_desc("Send/reply/edit/delete message")
                .category("Chat")
                .param("room", ParamType::String, true)
                .param("message", ParamType::String, false)
                .param("reply_to", ParamType::String, false)
                .param("edit", ParamType::String, false)
                .param("delete", ParamType::String, false)
                .param("notice", ParamType::Boolean, false)
                .param("report_later_here", ParamType::Boolean, false)
                .param("image", ParamType::String, false)
                .build(),
            ToolDefinition::from_yaml("Chat_react", t)
                .short_desc("React to a message")
                .category("Chat")
                .param("room", ParamType::String, true)
                .param("event_id", ParamType::String, true)
                .param("emoji", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("Chat_configure", t)
                .short_desc("Set room panel filters")
                .category("Chat")
                .param("room", ParamType::String, true)
                .param("n_messages", ParamType::Integer, false)
                .param("max_age", ParamType::String, false)
                .param("query", ParamType::String, false)
                .build(),
            ToolDefinition::from_yaml("Chat_search", t)
                .short_desc("Cross-room search")
                .category("Chat")
                .param("query", ParamType::String, true)
                .param("room", ParamType::String, false)
                .build(),
            ToolDefinition::from_yaml("Chat_create_room", t)
                .short_desc("Create a new room")
                .category("Chat")
                .param("name", ParamType::String, true)
                .param("topic", ParamType::String, false)
                .param_array("invite", ParamType::String, false)
                .build(),
            ToolDefinition::from_yaml("Chat_invite", t)
                .short_desc("Invite user to room")
                .category("Chat")
                .param("room", ParamType::String, true)
                .param("user_id", ParamType::String, true)
                .build(),
        ]
    }

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<Verdict> {
        match tool.name.as_str() {
            "Chat_open" | "Chat_send" | "Chat_react" | "Chat_configure" | "Chat_search" | "Chat_create_room"
            | "Chat_invite" => {
                let mut pf = Verdict::new();
                let cs = ChatState::get(state);
                if cs.server_status == types::ServerStatus::Stopped {
                    pf.errors.push("Chat server is not running. Activate the chat module first.".to_string());
                }
                if let types::ServerStatus::Error(ref e) = cs.server_status {
                    pf.warnings.push(format!("Chat server has an error: {e}"));
                }
                Some(pf)
            }
            _ => None,
        }
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "Chat_open" | "Chat_send" | "Chat_react" | "Chat_configure" | "Chat_search" | "Chat_create_room"
            | "Chat_invite" => Some(tools::dispatch(tool, state)),
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![
            cp_base::state::context::TypeMeta {
                context_type: "chat-dashboard",
                icon_id: "chat",
                is_fixed: true,
                needs_cache: false,
                fixed_order: Some(9),
                display_name: "chat",
                short_name: "chat",
                needs_async_wait: false,
            },
            cp_base::state::context::TypeMeta {
                context_type: "chat:room",
                icon_id: "chat",
                is_fixed: false,
                needs_cache: false,
                fixed_order: None,
                display_name: "chat room",
                short_name: "room",
                needs_async_wait: false,
            },
        ]
    }

    fn overview_context_section(&self, state: &State) -> Option<String> {
        let cs = ChatState::get(state);
        let status_label = match &cs.server_status {
            types::ServerStatus::Stopped => "stopped",
            types::ServerStatus::Starting => "starting",
            types::ServerStatus::Running => "running",
            types::ServerStatus::Error(_) => "error",
        };
        let mut section = format!("Chat: {status_label}");
        if !cs.rooms.is_empty() {
            {
                let _r = write!(section, ", {} rooms", cs.rooms.len());
            }
            // Show bridge breakdown if any bridged rooms exist
            let bridged: usize = cs.rooms.iter().filter(|r| r.bridge_source.is_some()).count();
            if bridged > 0 {
                let _r = write!(section, " ({bridged} bridged)");
            }
        }
        let total_unread: u64 = cs.rooms.iter().map(|r| r.unread_count).sum();
        if total_unread > 0 {
            {
                let _r = write!(section, ", {total_unread} unread");
            }
        }
        section.push('\n');
        Some(section)
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Chat", "Matrix-based messaging across Telegram, Discord, Slack, and Google Chat")]
    }

    fn context_display_name(&self, context_type: &str) -> Option<&'static str> {
        match context_type {
            "chat-dashboard" => Some("Chat"),
            _ => None,
        }
    }

    fn context_detail(&self, _ctx: &cp_base::state::context::Entry) -> Option<String> {
        None
    }

    fn overview_render_sections(&self, _state: &State) -> Vec<(u8, Vec<cp_render::Block>)> {
        vec![]
    }

    fn on_close_context(
        &self,
        ctx: &cp_base::state::context::Entry,
        state: &mut State,
    ) -> Option<Result<String, String>> {
        // Clean up open room state when a room panel is closed
        if ctx.context_type.as_str() == "chat:room"
            && let Some(room_id) = ctx.get_meta_str("room_id")
        {
            let cs = ChatState::get_mut(state);
            let _removed = cs.open_rooms.remove(room_id);
        }
        None
    }

    fn on_user_message(&self, _state: &mut State) {}

    fn on_stream_stop(&self, state: &mut State) {
        // Clear any active typing indicator when the stream is interrupted
        clear_typing_indicator(state);
    }

    fn on_tool_progress(&self, tool_name: &str, input_so_far: &str, state: &mut State) {
        if tool_name != "Chat_send" {
            return;
        }
        // Try to parse the room param from partial JSON streaming
        let room_ref = extract_room_from_partial_json(input_so_far);
        let Some(room_ref) = room_ref else {
            return;
        };
        // Resolve to a room ID and send typing indicator
        let Ok(room_id) = client::resolve_room(&room_ref) else {
            return;
        };
        let room_id_str = room_id.to_string();

        let cs = ChatState::get_mut(state);
        // Only send if not already typing in this room
        if cs.typing_room.as_deref() == Some(room_id_str.as_str()) {
            return;
        }
        // Clear previous room's typing if we switched rooms
        if cs.typing_room.is_some() {
            clear_typing_indicator(state);
        }
        client::send::set_typing(&room_id_str, true);
        ChatState::get_mut(state).typing_room = Some(room_id_str);
    }

    fn on_tool_complete(&self, tool_name: &str, state: &mut State) {
        if tool_name == "Chat_send" || tool_name == "Chat_react" {
            clear_typing_indicator(state);
        }
    }

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

/// Clear any active typing indicator in the chat state.
fn clear_typing_indicator(state: &mut State) {
    let cs = ChatState::get_mut(state);
    if let Some(room_id) = cs.typing_room.take() {
        client::send::set_typing(&room_id, false);
    }
}

/// Recover running bridge processes from a previous session.
///
/// Scans all known bridges for PID files left by a prior invocation.
/// If the process is still alive and healthy, updates `bridge_status`
/// to `Running` so the dashboard reflects reality.
fn recover_bridges(state: &mut State) {
    let cs = ChatState::get_mut(state);
    for spec in bridges::BRIDGES {
        if bridges::lifecycle::binary_path(spec.name).is_some_and(|p| p.exists()) {
            match bridges::lifecycle::start(spec.name) {
                Ok(pid) => {
                    let _inserted =
                        cs.bridge_status.insert(spec.name.to_string(), types::BridgeStatus::Running { pid });

                    // Attempt bot login for command-based bridges (Discord, Slack)
                    if !spec.config_login
                        && let Err(e) = bridges::login::ensure_bridge_login(spec.name)
                    {
                        log::debug!("Bridge {} auto-login skipped: {e}", spec.name);
                    }
                }
                Err(e) => {
                    log::debug!("Bridge {} not recovered: {e}", spec.name);
                }
            }
        }
    }
}

/// Disconnect from bridge processes during module deactivation.
///
/// Does **not** stop the bridge processes — they are global resources
/// shared across all Context Pilot instances. Only clears this
/// instance's tracking state.
fn shutdown_bridges(state: &mut State) {
    let cs = ChatState::get_mut(state);
    cs.bridge_status.clear();
}

/// Extract the `"room"` value from a partial JSON string.
///
/// The LLM streams tool parameters incrementally, so the JSON may be
/// incomplete. We look for `"room":"<value>"` or `"room": "<value>"`
/// using a simple substring scan rather than a full parser.
fn extract_room_from_partial_json(input: &str) -> Option<String> {
    // Look for "room" key followed by a string value
    let room_key = input.find("\"room\"")?;
    let after_key = input.get(room_key.saturating_add(6)..)?;
    // Skip optional whitespace and colon
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let after_colon = after_colon.trim_start();
    // Parse the string value
    let after_quote = after_colon.strip_prefix('"')?;
    let end_quote = after_quote.find('"')?;
    let value = after_quote.get(..end_quote)?;
    if value.is_empty() { None } else { Some(value.to_string()) }
}
