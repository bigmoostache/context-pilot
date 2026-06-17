//! JSON-RPC 2.0 and Model Context Protocol wire types.
//!
//! Pure serialization layer — no I/O. MCP rides JSON-RPC 2.0: requests carry an
//! `id`, notifications omit it, and `tools/*` results map ~1:1 onto Anthropic's
//! `input_schema` once bridged.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Protocol version advertised in the `initialize` handshake. Servers echo their
/// own supported version back; the client tolerates a mismatch.
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// Client identity sent during `initialize`.
pub const CLIENT_NAME: &str = "context-pilot";

/// Outbound JSON-RPC request (carries an `id`, expects a response).
#[derive(Debug, Clone, Serialize)]
pub struct Request {
    /// Always `"2.0"`.
    pub jsonrpc: &'static str,
    /// Correlation id, unique per in-flight request.
    pub id: u64,
    /// RPC method name (e.g. `"tools/list"`).
    pub method: String,
    /// Method parameters; omitted from the wire when `null`.
    #[serde(skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

impl Request {
    /// Build a request with the canonical `"2.0"` tag.
    #[must_use]
    pub fn new<M: Into<String>>(id: u64, method: M, params: Value) -> Self {
        Self { jsonrpc: "2.0", id, method: method.into(), params }
    }
}

/// Outbound JSON-RPC notification (no `id`, no response expected).
#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    /// Always `"2.0"`.
    pub jsonrpc: &'static str,
    /// RPC method name (e.g. `"notifications/initialized"`).
    pub method: String,
    /// Method parameters; omitted from the wire when `null`.
    #[serde(skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

impl Notification {
    /// Build a notification with the canonical `"2.0"` tag.
    #[must_use]
    pub fn new<M: Into<String>>(method: M, params: Value) -> Self {
        Self { jsonrpc: "2.0", method: method.into(), params }
    }
}

/// Inbound JSON-RPC message. A response pairs `id` with `result` xor `error`;
/// a server-initiated notification has a `method` and no `id`.
#[derive(Debug, Clone, Deserialize)]
pub struct Incoming {
    /// Correlation id — present on responses, absent on notifications.
    #[serde(default)]
    pub id: Option<u64>,
    /// Success payload (mutually exclusive with `error`).
    #[serde(default)]
    pub result: Option<Value>,
    /// Failure payload (mutually exclusive with `result`).
    #[serde(default)]
    pub error: Option<RpcError>,
    /// Method name — present on server notifications/requests, absent on responses.
    #[serde(default)]
    pub method: Option<String>,
}

impl Incoming {
    /// True when this message is a response to one of our requests (has an `id`).
    #[must_use]
    pub const fn is_response(&self) -> bool {
        self.id.is_some()
    }
}

/// JSON-RPC error object.
#[derive(Debug, Clone, Deserialize)]
pub struct RpcError {
    /// Numeric error code (JSON-RPC reserved range or server-defined).
    pub code: i64,
    /// Human-readable message.
    pub message: String,
    /// Optional structured detail.
    #[serde(default)]
    pub data: Option<Value>,
}

impl core::fmt::Display for RpcError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "JSON-RPC error {}: {}", self.code, self.message)
    }
}

/// Capability handshake parameters (`initialize`).
#[derive(Debug, Clone, Serialize)]
pub struct InitializeParams {
    /// Protocol version the client speaks.
    #[serde(rename = "protocolVersion")]
    pub protocol_version: &'static str,
    /// Client-side capabilities — empty object for a bare tools client.
    pub capabilities: Value,
    /// Client identity.
    #[serde(rename = "clientInfo")]
    pub client_info: ClientInfo,
}

impl InitializeParams {
    /// Build handshake params with this crate's identity and a current version.
    #[must_use]
    pub fn new() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            capabilities: serde_json::json!({}),
            client_info: ClientInfo { name: CLIENT_NAME, version: env!("CARGO_PKG_VERSION") },
        }
    }
}

impl Default for InitializeParams {
    fn default() -> Self {
        Self::new()
    }
}

/// Client name/version pair reported during `initialize`.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ClientInfo {
    /// Client product name.
    pub name: &'static str,
    /// Client semantic version.
    pub version: &'static str,
}

/// Server identity returned by `initialize`.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerInfo {
    /// Server product name.
    #[serde(default)]
    pub name: String,
    /// Server version string.
    #[serde(default)]
    pub version: String,
}

/// Result payload of the `initialize` handshake.
#[derive(Debug, Clone, Deserialize)]
pub struct InitializeResult {
    /// Protocol version the server settled on.
    #[serde(rename = "protocolVersion", default)]
    pub protocol_version: String,
    /// Server identity.
    #[serde(rename = "serverInfo", default)]
    pub server_info: Option<ServerInfo>,
}

/// A tool advertised by an MCP server.
#[derive(Debug, Clone, Deserialize)]
pub struct Tool {
    /// Unique tool name (server-local).
    pub name: String,
    /// Human-readable description; absent on terse servers.
    #[serde(default)]
    pub description: Option<String>,
    /// JSON Schema for the tool's arguments.
    #[serde(rename = "inputSchema", default)]
    pub input_schema: Value,
}

/// Result payload of `tools/list`.
#[derive(Debug, Clone, Deserialize)]
pub struct ListToolsResult {
    /// Advertised tools.
    #[serde(default)]
    pub tools: Vec<Tool>,
}

/// A single content block in a `tools/call` result.
#[derive(Debug, Clone, Deserialize)]
pub struct ContentBlock {
    /// Block kind (`"text"`, `"image"`, `"resource"`, ...).
    #[serde(rename = "type", default)]
    pub kind: String,
    /// Text payload for `"text"` blocks.
    #[serde(default)]
    pub text: Option<String>,
}

/// Result payload of `tools/call`.
#[derive(Debug, Clone, Deserialize)]
pub struct CallToolResult {
    /// Ordered content blocks the tool produced.
    #[serde(default)]
    pub content: Vec<ContentBlock>,
    /// Whether the server flagged the call as an error.
    #[serde(rename = "isError", default)]
    pub is_error: bool,
}

impl CallToolResult {
    /// Concatenate all text blocks into a single string (drops non-text blocks).
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        for block in &self.content {
            if let Some(text) = &block.text {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(text);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serializes_with_jsonrpc_tag() {
        let req = Request::new(7, "tools/list", Value::Null);
        let json = serde_json::to_value(&req).expect("serialize");
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["id"], 7);
        assert_eq!(json["method"], "tools/list");
        // Null params are omitted entirely.
        assert!(json.get("params").is_none());
    }

    #[test]
    fn request_keeps_non_null_params() {
        let req = Request::new(1, "tools/call", serde_json::json!({ "name": "search" }));
        let json = serde_json::to_value(&req).expect("serialize");
        assert_eq!(json["params"]["name"], "search");
    }

    #[test]
    fn notification_has_no_id_field() {
        let note = Notification::new("notifications/initialized", Value::Null);
        let json = serde_json::to_value(&note).expect("serialize");
        assert!(json.get("id").is_none());
        assert_eq!(json["method"], "notifications/initialized");
    }

    #[test]
    fn incoming_response_is_detected() {
        let raw = r#"{"jsonrpc":"2.0","id":3,"result":{"ok":true}}"#;
        let msg: Incoming = serde_json::from_str(raw).expect("parse");
        assert!(msg.is_response());
        assert_eq!(msg.id, Some(3));
        assert!(msg.error.is_none());
    }

    #[test]
    fn incoming_notification_has_no_id() {
        let raw = r#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#;
        let msg: Incoming = serde_json::from_str(raw).expect("parse");
        assert!(!msg.is_response());
        assert_eq!(msg.method.as_deref(), Some("notifications/tools/list_changed"));
    }

    #[test]
    fn incoming_error_parses() {
        let raw = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#;
        let msg: Incoming = serde_json::from_str(raw).expect("parse");
        let err = msg.error.expect("error present");
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "Method not found");
    }

    #[test]
    fn list_tools_result_parses() {
        let raw = r#"{"tools":[{"name":"echo","description":"Echoes input","inputSchema":{"type":"object"}}]}"#;
        let parsed: ListToolsResult = serde_json::from_str(raw).expect("parse");
        assert_eq!(parsed.tools.len(), 1);
        assert_eq!(parsed.tools[0].name, "echo");
        assert_eq!(parsed.tools[0].description.as_deref(), Some("Echoes input"));
    }

    #[test]
    fn call_result_concatenates_text_blocks() {
        let raw = r#"{"content":[{"type":"text","text":"line one"},{"type":"text","text":"line two"}]}"#;
        let parsed: CallToolResult = serde_json::from_str(raw).expect("parse");
        assert!(!parsed.is_error);
        assert_eq!(parsed.text(), "line one\nline two");
    }

    #[test]
    fn call_result_flags_error() {
        let raw = r#"{"content":[{"type":"text","text":"boom"}],"isError":true}"#;
        let parsed: CallToolResult = serde_json::from_str(raw).expect("parse");
        assert!(parsed.is_error);
    }

    #[test]
    fn initialize_result_tolerates_missing_server_info() {
        let raw = r#"{"protocolVersion":"2024-11-05"}"#;
        let parsed: InitializeResult = serde_json::from_str(raw).expect("parse");
        assert_eq!(parsed.protocol_version, "2024-11-05");
        assert!(parsed.server_info.is_none());
    }
}
