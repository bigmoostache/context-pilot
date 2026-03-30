//! Chat module — Matrix-based universal messaging layer.
//!
//! Provides 8 tools (`Chat_open`, `Chat_send`, `Chat_react`, `Chat_configure`,
//! `Chat_search`, `Chat_mark_as_read`, `Chat_create_room`, `Chat_invite`) and
//! 2 panel types (`ChatDashboardPanel`, `ChatRoomPanel`) backed by a local
//! Matrix homeserver (Tuwunel) with transparent bridge support.

/// First-run bootstrap: directory layout, config generation, credential scaffolding.
mod bootstrap;
/// Matrix SDK client wrapper: connection, authentication, sync loop, sending.
mod client;
/// Panel rendering: room panels and dashboard.
mod panels;
/// Tuwunel homeserver process lifecycle: start, stop, health check.
mod server;
/// Async-to-sync event bridge: channel, drain, Spine notification coalescing.
mod sync;
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
        "Matrix-based universal messaging (Discord, WhatsApp, Telegram, etc.)"
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
        let root = std::path::Path::new(".");
        if let Err(e) = bootstrap::bootstrap(root) {
            let cs = ChatState::get_mut(state);
            cs.server_status = types::ServerStatus::Error(format!("Bootstrap failed: {e}"));
            return;
        }

        // Start the homeserver, then connect the Matrix client
        if let Err(e) = server::start_server(state) {
            log::warn!("Chat server failed to start: {e}");
            return;
        }

        if let Err(e) = client::connect() {
            log::warn!("Matrix client connection failed: {e}");
            return;
        }

        client::start_sync();
    }

    fn reset_state(&self, state: &mut State) {
        // Tear down in reverse order: client → server → state
        client::disconnect();
        server::stop_server(state);
        state.set_ext(ChatState::default());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let cs = ChatState::get(state);
        json!({
            "search_query": cs.search_query,
            "server_pid": cs.server_pid,
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
            ToolDefinition::from_yaml("Chat_mark_as_read", t)
                .short_desc("Acknowledge room messages")
                .category("Chat")
                .param("room", ParamType::String, true)
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
            "Chat_send" | "Chat_react" | "Chat_configure" | "Chat_mark_as_read" => {
                let mut pf = Verdict::new();
                let cs = ChatState::get(state);
                if cs.server_status == types::ServerStatus::Stopped {
                    pf.errors.push("Chat server is not running. Activate the chat module first.".to_string());
                }
                Some(pf)
            }
            _ => None,
        }
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "Chat_open" | "Chat_send" | "Chat_react" | "Chat_configure" | "Chat_search" | "Chat_mark_as_read"
            | "Chat_create_room" | "Chat_invite" => Some(tools::dispatch(tool, state)),
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
        vec![("Chat", "Matrix-based messaging across Discord, WhatsApp, Telegram, and more")]
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

    fn overview_render_sections(
        &self,
        _state: &State,
        _base_style: ratatui::prelude::Style,
    ) -> Vec<(u8, Vec<ratatui::text::Line<'static>>)> {
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

    fn on_stream_stop(&self, _state: &mut State) {}

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
