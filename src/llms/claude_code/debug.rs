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

