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
use self::servers::{ConnStatus, McpServerEntry, McpState};

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
                if entry.is_tool_exposed(&tool.name) {
                    defs.push(tools::tool_definition(&name, tool));
                }
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
            let mut entry = if let Some((command, args)) = spec.stdio() {
                Self::connect_stdio(command, args)
            } else if let Some(url) = spec.url.as_deref() {
                let token = spec.bearer_token.as_deref().unwrap_or("");
                Self::connect_http(url, token)
            } else {
                McpServerEntry::failed("no 'command' or 'url' in spec")
            };
            entry.spec = Some(spec.clone());
            entry.allow_tools.clone_from(&spec.allow_tools);
            entry.deny_tools.clone_from(&spec.deny_tools);
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
    ///
    /// On transport failure, marks the server as [`ConnStatus::Failed`]. On the
    /// next call to a failed server, attempts to reconnect using the stored spec.
    fn dispatch(tool: &ToolUse, state: &mut State) -> ToolResult {
        let Some((server, tool_name)) = tools::split_id(&tool.name) else {
            return ToolResult::with_name(
                tool.id.clone(),
                format!("'{}' is not a namespaced MCP tool", tool.name),
                true,
                tool.name.clone(),
            );
        };
        let args = tools::strip_metadata(tool.input.clone());

        // Phase 1: auto-reconnect if the server previously failed.
        Self::maybe_reconnect(server, state);

        // Phase 2: execute the call (borrows state immutably for the duration).
        let (outcome, tools_changed) = {
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
            match client_lock.lock() {
                Ok(mut client) => {
                    let result = client.call_tool(tool_name, &args);
                    let changed = client.take_tools_changed();
                    (result, changed)
                }
                Err(_poison) => {
                    return ToolResult::with_name(
                        tool.id.clone(),
                        format!("MCP server '{server}' client lock poisoned"),
                        true,
                        tool.name.clone(),
                    );
                }
            }
        };

        // Phase 3: on transport error, mark the server as failed for next time.
        let tool_result = match outcome {
            Ok(result) => tools::call_result_to_tool_result(tool.id.clone(), tool.name.clone(), &result),
            Err(e) => {
                let error_msg = e.to_string();
                let mcp = McpState::get_mut(state);
                if let Some(entry) = mcp.servers.get_mut(server) {
                    entry.status = ConnStatus::Failed(error_msg.clone());
                    entry.client = None;
                }
                ToolResult::with_name(
                    tool.id.clone(),
                    format!("MCP call '{}' failed: {error_msg}", tool.name),
                    true,
                    tool.name.clone(),
                )
            }
        };

        // Phase 4: refresh tool list if the server signaled tools/list_changed.
        if tools_changed {
            let refreshed = {
                let mcp = McpState::get(state);
                mcp.servers
                    .get(server)
                    .and_then(|e| e.client.as_ref())
                    .and_then(|lock| lock.lock().ok())
                    .and_then(|mut client| client.list_tools().ok().map(<[_]>::to_vec))
            };
            if let Some(new_tools) = refreshed {
                let mcp = McpState::get_mut(state);
                if let Some(entry) = mcp.servers.get_mut(server) {
                    entry.status = ConnStatus::Connected { tool_count: new_tools.len() };
                    entry.tools = new_tools;
                }
            }
        }

        tool_result
    }

    /// If the server is in [`ConnStatus::Failed`] and carries a spec, try to
    /// reconnect. Replaces the entry in-place on success; on failure, updates
    /// the error message.
    fn maybe_reconnect(server: &str, state: &mut State) {
        let mcp = McpState::get_mut(state);
        let Some(entry) = mcp.servers.get(server) else { return };
        if entry.status.is_connected() {
            return;
        }
        let Some(spec) = entry.spec.clone() else { return };
        let allow = entry.allow_tools.clone();
        let deny = entry.deny_tools.clone();

        let mut fresh = if let Some((cmd, args)) = spec.stdio() {
            Self::connect_stdio(cmd, args)
        } else if let Some(url) = spec.url.as_deref() {
            let token = spec.bearer_token.as_deref().unwrap_or("");
            Self::connect_http(url, token)
        } else {
            return;
        };
        fresh.spec = Some(spec);
        fresh.allow_tools = allow;
        fresh.deny_tools = deny;
        let _prev = mcp.servers.insert(server.to_owned(), fresh);
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

    fn overview_context_section(&self, state: &State) -> Option<String> {
        use std::fmt::Write as _;

        let mcp = McpState::get(state);
        if mcp.servers.is_empty() {
            return None;
        }

        let mut parts = Vec::new();
        for name in mcp.sorted_names() {
            let Some(entry) = mcp.servers.get(&name) else { continue };
            if entry.status.is_connected() {
                parts.push(format!("{name} ({} tools)", entry.tools.len()));
            } else {
                parts.push(format!("{name} ({})", entry.status.label()));
            }
        }

        let total = mcp.total_tools();
        let mut out = String::new();
        let _r = write!(
            out,
            "MCP servers: {}. {total} tools available but disabled — see MCP panel for catalog, use tool_manage to enable.",
            parts.join(", "),
        );
        Some(out)
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
