//! Host integration: the [`McpModule`] that wires MCP servers into Context Pilot.
//!
//! On startup, [`McpModule::init_state`] reads `.mcp.json`, spawns each stdio
//! server, runs the handshake, and caches the discovered tools in [`McpState`].
//! Discovered tools surface through [`dynamic_tool_definitions`](Module::dynamic_tool_definitions)
//! (folded into the tool list by the binary's `rebuild_tools`), and calls route
//! back to the owning server in [`execute_tool`](Module::execute_tool) on the
//! `{server}__{tool}` namespace.

/// `.mcp.json` discovery and parsing.
pub mod config;
/// MCP status panel.
pub mod panel;
/// Live host state: connected servers, tools, status.
pub mod servers;
/// MCP ↔ Context Pilot tool translation and dispatch helpers.
pub mod tools;

use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::context::{Kind, TypeMeta};
use cp_base::state::runtime::State;
use cp_base::tools::{ToolDefinition, ToolResult, ToolUse};

use self::panel::{MCP_KIND, McpPanel};
use self::servers::{McpServerEntry, McpState};

use crate::clients::{AnyClient, McpClient};

/// Host module bridging runtime-discovered MCP tools into Context Pilot.
#[derive(Debug, Clone, Copy)]
pub struct McpModule;

impl McpModule {
    /// Runtime-discovered tools across all connected servers, namespaced
    /// `{server}__{tool}`. Called by the binary's `rebuild_tools` (mirroring how
    /// the reverie's `optimize_context` is injected) — no `Module` trait method,
    /// since MCP is the sole source of dynamic tools.
    #[must_use]
    pub fn dynamic_tool_definitions(state: &State) -> Vec<ToolDefinition> {
        let mcp = McpState::get(state);
        let mut defs = Vec::new();
        for name in mcp.sorted_names() {
            let Some(entry) = mcp.servers.get(&name) else { continue };
            for tool in &entry.tools {
                defs.push(tools::tool_definition(&name, tool));
            }
        }
        defs
    }

    /// Read `.mcp.json` and connect every configured stdio server, recording the
    /// outcome (connected / failed / unsupported) per server in [`McpState`].
    fn connect_all(mcp: &mut McpState) {
        let cfg = match config::load() {
            Ok(c) => c,
            Err(_e) => return, // No/invalid config → no servers; surfaced as empty panel.
        };

        let mut names: Vec<String> = cfg.servers.keys().cloned().collect();
        names.sort();
        for name in names {
            let Some(spec) = cfg.servers.get(&name) else { continue };
            let entry = if let Some((command, args)) = spec.stdio() {
                Self::connect_stdio(command, args)
            } else if let Some(url) = spec.url.as_deref() {
                let token = spec.bearer_token.as_deref().unwrap_or("");
                Self::connect_http(url, token)
            } else {
                McpServerEntry::failed("no 'command' or 'url' in spec")
            };
            let _prev = mcp.servers.insert(name, entry);
        }
    }

    /// Spawn one stdio server, handshake, and fetch its tool list.
    fn connect_stdio(command: &str, args: &[String]) -> McpServerEntry {
        let mut client = match McpClient::connect_stdio(command, args) {
            Ok(c) => c,
            Err(e) => return McpServerEntry::failed(e.to_string()),
        };
        match client.list_tools() {
            Ok(tools) => {
                let tools = tools.to_vec();
                McpServerEntry::connected(AnyClient::Stdio(client), tools)
            }
            Err(e) => McpServerEntry::failed(e.to_string()),
        }
    }

    /// Connect to one remote HTTP/SSE server, handshake, and fetch its tool list.
    fn connect_http(url: &str, token: &str) -> McpServerEntry {
        let mut client = match McpClient::connect_http(url, token) {
            Ok(c) => c,
            Err(e) => return McpServerEntry::failed(e.to_string()),
        };
        match client.list_tools() {
            Ok(tools) => {
                let tools = tools.to_vec();
                McpServerEntry::connected(AnyClient::Http(client), tools)
            }
            Err(e) => McpServerEntry::failed(e.to_string()),
        }
    }

    /// Route a `{server}__{tool}` call to its server and return the formatted result.
    fn dispatch(tool: &ToolUse, state: &State) -> ToolResult {
        let Some((server, tool_name)) = tools::split_id(&tool.name) else {
            return ToolResult::with_name(
                tool.id.clone(),
                format!("'{}' is not a namespaced MCP tool", tool.name),
                true,
                tool.name.clone(),
            );
        };
        let args = tools::strip_metadata(tool.input.clone());

        let mcp = McpState::get(state);
        let Some(entry) = mcp.servers.get(server) else {
            return ToolResult::with_name(
                tool.id.clone(),
                format!("MCP server '{server}' is not registered"),
                true,
                tool.name.clone(),
            );
        };
        let Some(client_lock) = entry.client.as_ref() else {
            return ToolResult::with_name(
                tool.id.clone(),
                format!("MCP server '{server}' is not connected ({})", entry.status.label()),
                true,
                tool.name.clone(),
            );
        };

        let outcome = match client_lock.lock() {
            Ok(mut client) => client.call_tool(tool_name, &args),
            Err(_poison) => {
                return ToolResult::with_name(
                    tool.id.clone(),
                    format!("MCP server '{server}' client lock poisoned"),
                    true,
                    tool.name.clone(),
                );
            }
        };

        match outcome {
            Ok(result) => tools::call_result_to_tool_result(tool.id.clone(), tool.name.clone(), &result),
            Err(e) => ToolResult::with_name(
                tool.id.clone(),
                format!("MCP call '{}' failed: {e}", tool.name),
                true,
                tool.name.clone(),
            ),
        }
    }
}

impl Module for McpModule {
    fn id(&self) -> &'static str {
        "mcp"
    }
    fn name(&self) -> &'static str {
        "MCP"
    }
    fn description(&self) -> &'static str {
        "Connect to MCP servers and expose their tools"
    }

    fn init_state(&self, state: &mut State) {
        let mut mcp = McpState::default();
        Self::connect_all(&mut mcp);
        state.set_ext(mcp);
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(McpState::default());
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        // All MCP tools are runtime-discovered — none are static.
        vec![]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        // Own any namespaced tool whose server we know about.
        let (server, _rest) = tools::split_id(&tool.name)?;
        if !McpState::get(state).servers.contains_key(server) {
            return None;
        }
        Some(Self::dispatch(tool, state))
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        (context_type.as_str() == MCP_KIND).then(|| {
            let panel: Box<dyn Panel> = Box::new(McpPanel);
            panel
        })
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(MCP_KIND)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![(Kind::new(MCP_KIND), "MCP", false)]
    }

    fn context_type_metadata(&self) -> Vec<TypeMeta> {
        vec![TypeMeta {
            context_type: MCP_KIND,
            icon_id: "tmux",
            is_fixed: true,
            needs_cache: false,
            fixed_order: Some(20),
            display_name: "mcp",
            short_name: "mcp",
            needs_async_wait: false,
        }]
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("MCP", "Tools exposed by connected MCP servers")]
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

    fn save_module_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_module_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn pre_flight(&self, _tool: &ToolUse, _state: &State) -> Option<cp_base::tools::pre_flight::Verdict> {
        None
    }

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, cp_base::modules::ToolVisualizer)> {
        vec![]
    }

    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
        None
    }

    fn context_detail(&self, _ctx: &cp_base::state::context::Entry) -> Option<String> {
        None
    }

    fn overview_context_section(&self, _state: &State) -> Option<String> {
        None
    }

    fn overview_render_sections(&self, _state: &State) -> Vec<(u8, Vec<cp_render::Block>)> {
        vec![]
    }

    fn on_close_context(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _state: &mut State,
    ) -> Option<Result<String, String>> {
        None
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
