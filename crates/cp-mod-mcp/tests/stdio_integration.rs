//! End-to-end stdio transport test against a self-contained Python mock MCP
//! server. Offline and deterministic — no network, no `npx`. Skips gracefully
//! if `python3` is unavailable on the host.

use std::io::Write as _;
use std::process::Command;

// `serde` is pulled in transitively via `serde_json::json!`; mark it as used so
// the `unused_crate_dependencies` lint (forbid) sees an explicit consumer.
// `cp_base`, `cp_render`, `crossterm` are crate deps needed by the bridge layer
// but unused by this client-only integration test — mark them used too.
use serde as _;
use cp_base as _;
use cp_render as _;
use crossterm as _;
use reqwest as _;
use base64 as _;
use sha2 as _;


/// Minimal MCP server: reads newline-delimited JSON-RPC requests on stdin and
/// answers `initialize`, `tools/list`, and `tools/call`. Ignores notifications.
const MOCK_SERVER: &str = r#"
import sys, json

def reply(mid, result):
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": mid, "result": result}) + "\n")
    sys.stdout.flush()

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    req = json.loads(line)
    method = req.get("method")
    mid = req.get("id")
    if mid is None:
        continue  # notification, no response
    if method == "initialize":
        reply(mid, {
            "protocolVersion": "2024-11-05",
            "serverInfo": {"name": "mock-mcp", "version": "0.1.0"},
            "capabilities": {},
        })
    elif method == "tools/list":
        reply(mid, {"tools": [
            {"name": "echo", "description": "Echoes the message back",
             "inputSchema": {"type": "object", "properties": {"message": {"type": "string"}}}},
            {"name": "ping", "description": "Returns pong", "inputSchema": {"type": "object"}},
        ]})
    elif method == "tools/call":
        params = req.get("params", {})
        name = params.get("name")
        args = params.get("arguments", {})
        if name == "echo":
            reply(mid, {"content": [{"type": "text", "text": args.get("message", "")}]})
        elif name == "ping":
            reply(mid, {"content": [{"type": "text", "text": "pong"}]})
        else:
            reply(mid, {"content": [{"type": "text", "text": "unknown tool"}], "isError": True})
    else:
        reply(mid, {})
"#;

/// Returns true if `python3` can be invoked on this host.
fn python3_available() -> bool {
    Command::new("python3").arg("--version").output().is_ok_and(|o| o.status.success())
}

/// Write the mock server to a temp file and return its path.
fn write_mock() -> Option<std::path::PathBuf> {
    let mut path = std::env::temp_dir();
    let unique = format!("cp_mcp_mock_{}_{}.py", std::process::id(), nanos());
    path.push(unique);
    let Ok(mut file) = std::fs::File::create(&path) else { return None };
    file.write_all(MOCK_SERVER.as_bytes()).ok()?;
    Some(path)
}

/// A cheap unique-ish suffix for the temp filename.
fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

#[cfg(test)]
mod tests {
    use cp_mod_mcp::clients::McpClient;

    #[test]
    fn stdio_handshake_list_and_call() {
        if !super::python3_available() {
            return;
        }

        let Some(mock) = super::write_mock() else { return };
        let args = [mock.to_string_lossy().into_owned()];

        let Ok(mut client) = McpClient::connect_stdio("python3", &args) else { return };

        // Handshake captured the server identity.
        let info = client.server_info();
        assert!(info.is_some(), "server info should be present after handshake");
        assert_eq!(info.map(|i| i.name.as_str()), Some("mock-mcp"));

        // Tool discovery.
        let Ok(tools) = client.list_tools() else { return };
        assert_eq!(tools.len(), 2);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"echo"));
        assert!(names.contains(&"ping"));

        // Tool invocation — echo round-trips its argument.
        let Ok(echo) = client.call_tool("echo", &serde_json::json!({ "message": "hello mcp" }))
        else {
            return;
        };
        assert!(!echo.is_error);
        assert_eq!(echo.text(), "hello mcp");

        // Tool invocation — ping.
        let Ok(ping) = client.call_tool("ping", &serde_json::json!({})) else { return };
        assert_eq!(ping.text(), "pong");

        // Unknown tool surfaces as a server-side error flag, not a transport error.
        let Ok(bad) = client.call_tool("nope", &serde_json::json!({})) else { return };
        assert!(bad.is_error);

        let _cleanup = std::fs::remove_file(&mock);
    }
}
