//! Debug helpers for the Claude Code provider.

use std::fs;

use serde_json::Value;

use super::{CLAUDE_CODE_ENDPOINT, OAUTH_BETA_HEADER};
use crate::infra::constants::API_VERSION;

/// Directory for last-request debug dumps
const LAST_REQUESTS_DIR: &str = ".context-pilot/last_requests";

/// Dump the outgoing API request to disk for debugging.
/// Written to `.context-pilot/last_requests/{worker_id}_last_request.json`.
pub(super) fn dump_last_request(worker_id: &str, api_request: &Value) {
    let debug = serde_json::json!({
        "request_url": CLAUDE_CODE_ENDPOINT,
        "request_headers": {
            "anthropic-beta": OAUTH_BETA_HEADER,
            "anthropic-version": API_VERSION,
            "user-agent": "claude-cli/2.1.37 (external, cli)",
            "x-app": "cli",
        },
        "request_body": api_request,
    });
    let _r1 = fs::create_dir_all(LAST_REQUESTS_DIR);
    let path = format!("{LAST_REQUESTS_DIR}/{worker_id}_last_request.json");
    let _r2 = fs::write(path, serde_json::to_string_pretty(&debug).unwrap_or_default());
}

/// Context for building verbose stream-read error diagnostics.
pub(super) struct StreamErrorContext<'ctx> {
    /// The I/O error that terminated the stream read.
    pub err: &'ctx std::io::Error,
    /// Tool currently being streamed, if any: `(id, name, partial_input)`.
    pub current_tool: Option<&'ctx (String, String, String)>,
    /// Total bytes read from the response body before failure.
    pub total_bytes: usize,
    /// Total SSE lines read before failure.
    pub line_count: usize,
    /// Formatted response headers for diagnostic output.
    pub resp_headers: &'ctx str,
    /// Last few SSE data lines for error context.
    pub last_lines: &'ctx [String],
}

/// Build a verbose error message for stream read failures.
/// Captures error kind, root cause chain, stream position, in-flight tool context,
/// response headers, and recent SSE lines.
pub(super) fn build_stream_read_error(ctx: &StreamErrorContext<'_>) -> String {
    let error_kind = format!("{:?}", ctx.err.kind());
    let mut root_cause = String::new();
    let mut source: Option<&dyn std::error::Error> = std::error::Error::source(ctx.err);
    while let Some(s) = source {
        root_cause = format!("{s}");
        source = std::error::Error::source(s);
    }
    let tool_ctx = ctx.current_tool.map_or_else(
        || "No tool in progress".to_owned(),
        |tool| format!("In-flight tool: {} (id={}), partial input: {} bytes", tool.1, tool.0, tool.2.len()),
    );
    let recent = if ctx.last_lines.is_empty() { "(no lines read)".to_owned() } else { ctx.last_lines.join("\n") };
    format!(
        "{}\n\
         Error kind: {error_kind} | Root cause: {}\n\
         Stream position: {} bytes, {} lines read\n\
         {tool_ctx}\n\
         Response headers:\n{}\n\
         Last SSE lines:\n{recent}",
        ctx.err,
        if root_cause.is_empty() { "(none)".to_owned() } else { root_cause },
        ctx.total_bytes,
        ctx.line_count,
        ctx.resp_headers,
    )
}
