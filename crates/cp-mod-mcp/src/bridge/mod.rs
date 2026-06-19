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
/// Form selector enums for the MCP setup overlay (server type, auth mode, scope).
pub mod form_enums;
/// MCP status panel.
pub mod panel;
/// Live host state: connected servers, tools, status.
pub mod servers;
/// Mutable form state for the MCP setup overlay.
pub mod setup;
/// MCP ↔ Context Pilot tool translation and dispatch helpers.
pub mod tools;
/// [`Module`] trait implementation (separated for file-size hygiene).
mod module_impl;

use cp_base::state::runtime::State;
use cp_base::tools::{ToolDefinition, ToolResult, ToolUse};

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
            } else if spec.url.is_some() {
                Self::connect_url(spec.url.as_deref().unwrap_or_default(), spec)
            } else {
                McpServerEntry::failed("no 'command' or 'url' in spec")
            };
            entry.spec = Some(spec.clone());
            entry.allow_tools.clone_from(&spec.allow_tools);
            entry.deny_tools.clone_from(&spec.deny_tools);
            let _prev = mcp.servers.insert(name, entry);
        }
    }

    /// Connect to a URL-based server, resolving authentication through the
    /// `auth` field in the spec:
    /// - `"none"` → connect without any auth header
    /// - `"oauth"` → full OAuth 2.1 + PKCE browser flow
    /// - absent → auto: use `bearer_token` if present, else attempt OAuth
    fn connect_url(url: &str, spec: &config::ServerSpec) -> McpServerEntry {
        let auth_mode = spec.auth.as_deref().unwrap_or("auto");
        let token = match auth_mode {
            "none" => String::new(),
            "oauth" => match crate::oauth::authorize(url) {
                Ok(t) => t,
                Err(e) => return McpServerEntry::failed(format!("OAuth: {e}")),
            },
            _ => {
                // "auto" or unrecognized: bearer_token if present, else OAuth
                match spec.bearer_token.as_deref() {
                    Some(t) if !t.is_empty() => t.to_owned(),
                    _ => match crate::oauth::authorize(url) {
                        Ok(t) => t,
                        Err(e) => return McpServerEntry::failed(format!("OAuth: {e}")),
                    },
                }
            }
        };
        Self::connect_http(url, &token)
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
        } else if spec.url.is_some() {
            Self::connect_url(spec.url.as_deref().unwrap_or_default(), &spec)
        } else {
            return;
        };
        fresh.spec = Some(spec);
        fresh.allow_tools = allow;
        fresh.deny_tools = deny;
        let _prev = mcp.servers.insert(server.to_owned(), fresh);
    }

    // ── Public server management API (called from UI layer) ─────────────

    /// Add a new server: save to config and connect.
    ///
    /// `to_project` selects the config file:
    /// - `true`  → `.context-pilot/shared/mcp.json` (project-local)
    /// - `false` → `~/.context-pilot/mcp.json` (global)
    ///
    /// # Errors
    ///
    /// Returns a human-readable message on config I/O or serialization failure.
    pub fn add_and_connect(
        name: &str,
        spec: &config::ServerSpec,
        to_project: bool,
        state: &mut State,
    ) -> Result<(), String> {
        // 1. Save to config file
        let mut manifest = if to_project {
            config::load_project()?
        } else {
            config::load_global()?
        };
        let _saved = manifest.servers.insert(name.to_string(), spec.clone());
        if to_project {
            let _p = config::save_to_project(&manifest)?;
        } else {
            let _p = config::save_to_global(&manifest)?;
        }

        // 2. Connect the server
        let mut entry = if let Some((cmd, args)) = spec.stdio() {
            Self::connect_stdio(cmd, args)
        } else if spec.url.is_some() {
            Self::connect_url(spec.url.as_deref().unwrap_or_default(), spec)
        } else {
            McpServerEntry::failed("no 'command' or 'url' in spec")
        };
        entry.spec = Some(spec.clone());
        entry.allow_tools.clone_from(&spec.allow_tools);
        entry.deny_tools.clone_from(&spec.deny_tools);

        // 3. Add to runtime state
        let mcp = McpState::get_mut(state);
        let _inserted = mcp.servers.insert(name.to_string(), entry);

        Ok(())
    }

    /// Remove a server: delete from config files and disconnect.
    ///
    /// Checks both global and project configs, removing from whichever
    /// contains the server.
    ///
    /// # Errors
    ///
    /// Returns a human-readable message on config I/O failure.
    pub fn remove_and_disconnect(name: &str, state: &mut State) -> Result<(), String> {
        // Remove from global config if present
        let mut global = config::load_global()?;
        if global.servers.remove(name).is_some() {
            let _p = config::save_to_global(&global)?;
        }

        // Remove from project config if present
        let mut project = config::load_project()?;
        if project.servers.remove(name).is_some() {
            let _p = config::save_to_project(&project)?;
        }

        // Remove from runtime state (drops client)
        let mcp = McpState::get_mut(state);
        let _prev = mcp.servers.remove(name);

        Ok(())
    }

    /// Force-reconnect a server using its stored spec.
    ///
    /// Disconnects the existing client (if any) and re-connects from scratch.
    /// No-op if the server has no stored spec.
    pub fn force_reconnect(name: &str, state: &mut State) {
        let (spec, allow, deny) = {
            let mcp = McpState::get(state);
            match mcp.servers.get(name) {
                Some(entry) => (
                    entry.spec.clone(),
                    entry.allow_tools.clone(),
                    entry.deny_tools.clone(),
                ),
                None => return,
            }
        };
        let Some(spec) = spec else { return };

        let mut fresh = if let Some((cmd, args)) = spec.stdio() {
            Self::connect_stdio(cmd, args)
        } else if spec.url.is_some() {
            Self::connect_url(spec.url.as_deref().unwrap_or_default(), &spec)
        } else {
            return;
        };
        fresh.spec = Some(spec);
        fresh.allow_tools = allow;
        fresh.deny_tools = deny;

        let mcp = McpState::get_mut(state);
        let _prev = mcp.servers.insert(name.to_owned(), fresh);
    }
}
