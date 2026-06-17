//! Per-server MCP client: handshake, tool discovery, and tool invocation.
//!
//! Generic over [`Transport`], so the same logic drives stdio (Phase 1) and a
//! future HTTP transport. Requests are correlated by a monotonic id; the
//! response loop skips server-initiated notifications while awaiting the
//! matching reply.

use serde_json::Value;

use crate::errors::McpError;
use crate::protocol::{
    CallToolResult, Incoming, InitializeParams, InitializeResult, ListToolsResult, Notification, Request, ServerInfo,
    Tool,
};
use crate::transport::Transport;
use crate::transport::pipe::SubprocessTransport;
use crate::transport::streamable::HttpTransport;

/// Default per-request timeout. Generous enough for an `npx` server's cold start
/// on the first `initialize`, tight enough to fail fast on a hung server.
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// A connected MCP server.
///
/// Holds the transport, the next request id, and the most recent handshake /
/// tool-list snapshot.
#[derive(Debug)]
pub struct McpClient<T: Transport> {
    /// Underlying JSON-RPC channel.
    transport: T,
    /// Monotonic request id source.
    next_id: u64,
    /// Per-request timeout in milliseconds.
    timeout_ms: u64,
    /// Server identity from the handshake, if it reported one.
    server_info: Option<ServerInfo>,
    /// Most recent `tools/list` snapshot.
    tools: Vec<Tool>,
    /// Set when a `notifications/tools/list_changed` is received during a
    /// request. The bridge checks and clears this after each call.
    tools_changed: bool,
}

impl McpClient<SubprocessTransport> {
    /// Spawn a stdio MCP server and perform the `initialize` handshake.
    ///
    /// # Errors
    ///
    /// Propagates spawn, transport, timeout, or protocol failures.
    pub fn connect_stdio(command: &str, args: &[String]) -> Result<Self, McpError> {
        let transport = SubprocessTransport::spawn(command, args)?;
        let mut client = Self::with_transport(transport);
        let _handshake = client.initialize()?;
        Ok(client)
    }
}

impl McpClient<HttpTransport> {
    /// Connect to a remote MCP server over Streamable HTTP and perform the
    /// `initialize` handshake. Pass `token = ""` for an unauthenticated server;
    /// otherwise it is sent as an `Authorization: Bearer` header.
    ///
    /// # Errors
    ///
    /// Propagates transport, timeout, or protocol failures (including a non-2xx
    /// status such as `401 Unauthorized`).
    pub fn connect_http(url: &str, token: &str) -> Result<Self, McpError> {
        let transport = HttpTransport::new(url, token, DEFAULT_TIMEOUT_MS)?;
        let mut client = Self::with_transport(transport);
        let _handshake = client.initialize()?;
        Ok(client)
    }
}

impl<T: Transport> McpClient<T> {
    /// Wrap an already-constructed transport (used by `connect_*` and tests).
    #[must_use]
    pub const fn with_transport(transport: T) -> Self {
        Self { transport, next_id: 1, timeout_ms: DEFAULT_TIMEOUT_MS, server_info: None, tools: Vec::new(), tools_changed: false }
    }

    /// Override the per-request timeout.
    pub const fn set_timeout_ms(&mut self, timeout_ms: u64) {
        self.timeout_ms = timeout_ms;
    }

    /// Server identity reported during `initialize`, if any.
    #[must_use]
    pub const fn server_info(&self) -> Option<&ServerInfo> {
        self.server_info.as_ref()
    }

    /// Tools from the most recent [`list_tools`](Self::list_tools) call.
    #[must_use]
    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    /// Returns and clears the `tools_changed` flag set when a
    /// `notifications/tools/list_changed` was received during a request.
    pub fn take_tools_changed(&mut self) -> bool {
        std::mem::take(&mut self.tools_changed)
    }

    /// Run the `initialize` handshake and send `notifications/initialized`.
    ///
    /// Returns the decoded handshake result by value; the server identity is
    /// also cached and reachable via [`server_info`](Self::server_info).
    ///
    /// # Errors
    ///
    /// Propagates transport/timeout/protocol failures.
    pub fn initialize(&mut self) -> Result<InitializeResult, McpError> {
        let params = serde_json::to_value(InitializeParams::new())
            .map_err(|e| McpError::Protocol(format!("encode initialize params: {e}")))?;
        let result_value = self.request("initialize", params)?;
        let result: InitializeResult = serde_json::from_value(result_value)
            .map_err(|e| McpError::Protocol(format!("decode initialize result: {e}")))?;
        self.server_info.clone_from(&result.server_info);

        // Confirm readiness — fire-and-forget, no response expected.
        self.notify("notifications/initialized", Value::Null)?;

        Ok(result)
    }

    /// Fetch the server's advertised tools, refreshing the cached snapshot.
    ///
    /// # Errors
    ///
    /// Propagates transport/timeout/protocol failures.
    pub fn list_tools(&mut self) -> Result<&[Tool], McpError> {
        let result_value = self.request("tools/list", Value::Null)?;
        let parsed: ListToolsResult = serde_json::from_value(result_value)
            .map_err(|e| McpError::Protocol(format!("decode tools/list: {e}")))?;
        self.tools = parsed.tools;
        Ok(&self.tools)
    }

    /// Invoke a tool by name with JSON `arguments`.
    ///
    /// # Errors
    ///
    /// Propagates transport/timeout/protocol failures. A server-side tool error
    /// surfaces via [`CallToolResult::is_error`], not as an `Err`.
    pub fn call_tool(&mut self, name: &str, arguments: &Value) -> Result<CallToolResult, McpError> {
        let params = serde_json::json!({ "name": name, "arguments": arguments });
        let result_value = self.request("tools/call", params)?;
        serde_json::from_value(result_value).map_err(|e| McpError::Protocol(format!("decode tools/call: {e}")))
    }

    /// Send a request and await its matching response, skipping any interleaved
    /// server notifications. Returns the `result` value or maps a JSON-RPC
    /// `error` to [`McpError::Rpc`].
    fn request(&mut self, method: &str, params: Value) -> Result<Value, McpError> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        let req = Request::new(id, method, params);
        let line = serde_json::to_string(&req).map_err(|e| McpError::Protocol(format!("encode request: {e}")))?;
        self.transport.send_line(&line)?;

        loop {
            let msg: Incoming = self.transport.recv(self.timeout_ms)?;
            if !msg.is_response() {
                if msg.method.as_deref() == Some("notifications/tools/list_changed") {
                    self.tools_changed = true;
                }
                continue; // Server notification — not our reply.
            }
            if msg.id != Some(id) {
                continue; // Stale/out-of-order response for a different request.
            }
            if let Some(err) = msg.error {
                return Err(McpError::Rpc(err));
            }
            return msg.result.ok_or_else(|| McpError::Protocol("response missing result".to_owned()));
        }
    }

    /// Send a fire-and-forget notification (no id, no response).
    fn notify(&mut self, method: &str, params: Value) -> Result<(), McpError> {
        let note = Notification::new(method, params);
        let line = serde_json::to_string(&note).map_err(|e| McpError::Protocol(format!("encode notification: {e}")))?;
        self.transport.send_line(&line)
    }
}

// ---------------------------------------------------------------------------
// Transport-erased wrapper
// ---------------------------------------------------------------------------

/// Transport-erased MCP client.
///
/// Wraps either a stdio or HTTP client behind a uniform API so
/// [`McpServerEntry`](crate::bridge::servers::McpServerEntry) can hold either
/// variant without leaking the generic parameter.
#[derive(Debug)]
pub enum AnyClient {
    /// Subprocess (stdio) transport.
    Stdio(McpClient<SubprocessTransport>),
    /// Remote HTTP / SSE transport.
    Http(McpClient<HttpTransport>),
}

impl AnyClient {
    /// Invoke a tool by name with JSON `arguments`.
    ///
    /// Delegates to the underlying transport-specific client.
    ///
    /// # Errors
    ///
    /// Propagates transport/timeout/protocol failures.
    pub fn call_tool(&mut self, name: &str, arguments: &Value) -> Result<CallToolResult, McpError> {
        match self {
            Self::Stdio(client) => client.call_tool(name, arguments),
            Self::Http(client) => client.call_tool(name, arguments),
        }
    }

    /// Fetch the server's advertised tools, refreshing the cached snapshot.
    ///
    /// # Errors
    ///
    /// Propagates transport/timeout/protocol failures.
    pub fn list_tools(&mut self) -> Result<&[Tool], McpError> {
        match self {
            Self::Stdio(client) => client.list_tools(),
            Self::Http(client) => client.list_tools(),
        }
    }

    /// Server identity reported during `initialize`, if any.
    #[must_use]
    pub const fn server_info(&self) -> Option<&ServerInfo> {
        match self {
            Self::Stdio(client) => client.server_info(),
            Self::Http(client) => client.server_info(),
        }
    }

    /// Returns and clears the `tools_changed` flag.
    pub fn take_tools_changed(&mut self) -> bool {
        match self {
            Self::Stdio(client) => client.take_tools_changed(),
            Self::Http(client) => client.take_tools_changed(),
        }
    }
}
