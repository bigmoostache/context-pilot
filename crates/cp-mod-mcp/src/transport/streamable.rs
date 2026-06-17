//! Streamable HTTP transport: exchange JSON-RPC with an MCP server over a single
//! HTTP endpoint, with optional static bearer-token auth.
//!
//! Per the MCP Streamable HTTP spec, every client message is an HTTP `POST` to
//! one URL. The server answers either with a single `application/json` body (one
//! JSON-RPC message) or a `text/event-stream` (Server-Sent Events) body carrying
//! one or more messages. Notifications and responses that expect no reply get a
//! bare `202 Accepted`.
//!
//! This transport is *pull-based* to satisfy the synchronous [`Transport`]
//! contract: [`send_line`](HttpTransport::send_line) performs the POST and drains
//! the whole response into an in-memory inbox; [`recv`](HttpTransport::recv) then
//! hands back one buffered message at a time. That matches how
//! [`crate::clients::McpClient`] correlates a request with its response (send,
//! then loop `recv` skipping non-matching ids).
//!
//! Phase 3 covers request/response and short SSE bursts that the server closes
//! after the final reply. Long-lived server-push streams (`tools/list_changed`)
//! are a Phase 5 concern.

use std::collections::VecDeque;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};

use crate::errors::McpError;
use crate::protocol::Incoming;

use super::Transport;

/// Header the server uses to assign (and the client to echo) a session id.
const SESSION_HEADER: &str = "Mcp-Session-Id";

/// Content negotiation: accept either a JSON body or an SSE stream.
const ACCEPT_VALUE: &str = "application/json, text/event-stream";

/// Cap on the error-body snippet included in a failed-status [`McpError`].
const ERROR_SNIPPET_CHARS: usize = 200;

/// An MCP server addressed over Streamable HTTP.
#[derive(Debug)]
pub struct HttpTransport {
    /// Blocking HTTP client; its timeout bounds every POST.
    client: Client,
    /// Single endpoint every message is `POSTed` to.
    url: String,
    /// Bearer token; empty means send no `Authorization` header.
    token: String,
    /// Session id assigned by the server on `initialize`, echoed thereafter.
    session_id: Option<String>,
    /// Messages parsed from server responses, awaiting [`recv`](Self::recv).
    inbox: VecDeque<Incoming>,
}

impl HttpTransport {
    /// Build a transport for `url`, authenticating with `token` (pass `""` for
    /// an unauthenticated server). `timeout_ms` bounds each request.
    ///
    /// No network call happens here — the `initialize` handshake is driven by
    /// [`crate::clients::McpClient`] via the first [`send_line`](Self::send_line).
    ///
    /// # Errors
    ///
    /// Returns [`McpError::Transport`] if the HTTP client cannot be constructed.
    pub fn new(url: &str, token: &str, timeout_ms: u64) -> Result<Self, McpError> {
        let client = Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .build()
            .map_err(|e| McpError::Transport(format!("build HTTP client: {e}")))?;
        Ok(Self {
            client,
            url: url.to_owned(),
            token: token.to_owned(),
            session_id: None,
            inbox: VecDeque::new(),
        })
    }

    /// POST one serialized JSON-RPC message and enqueue every message the server
    /// returns (none for a `202`, one for JSON, many for SSE).
    fn post(&mut self, json: &str) -> Result<(), McpError> {
        let mut req = self
            .client
            .post(&self.url)
            .header(ACCEPT, ACCEPT_VALUE)
            .header(CONTENT_TYPE, "application/json")
            .body(json.to_owned());
        if !self.token.is_empty() {
            req = req.header(AUTHORIZATION, format!("Bearer {}", self.token));
        }
        if let Some(sid) = &self.session_id {
            req = req.header(SESSION_HEADER, sid.clone());
        }

        let resp = req.send().map_err(|e| McpError::Transport(format!("HTTP send: {e}")))?;

        // A server assigns the session id on the initialize response; cache it.
        if let Some(sid) = resp.headers().get(SESSION_HEADER).and_then(|v| v.to_str().ok()) {
            self.session_id = Some(sid.to_owned());
        }

        let status = resp.status();
        let is_sse = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.contains("text/event-stream"));

        if !status.is_success() {
            let body = resp.text().unwrap_or_default();
            let snippet: String = body.chars().take(ERROR_SNIPPET_CHARS).collect();
            return Err(McpError::Transport(format!("HTTP {status}: {snippet}")));
        }

        // 202 Accepted carries no body — a notification/response was acknowledged.
        if status == StatusCode::ACCEPTED {
            return Ok(());
        }

        let body = resp.text().map_err(|e| McpError::Transport(format!("HTTP body: {e}")))?;
        let messages = if is_sse { parse_sse(&body) } else { parse_json_messages(&body) };
        for msg in messages {
            self.inbox.push_back(msg);
        }
        Ok(())
    }
}

impl Transport for HttpTransport {
    fn send_line(&mut self, json: &str) -> Result<(), McpError> {
        self.post(json)
    }

    fn recv(&mut self, timeout_ms: u64) -> Result<Incoming, McpError> {
        // All I/O happened during `send_line`; there is nothing to wait on, so the
        // timeout is irrelevant — either a message is buffered or it never arrived.
        let _ = timeout_ms;
        self.inbox.pop_front().ok_or(McpError::Timeout)
    }
}

/// Parse a non-SSE response body. Accepts a single JSON-RPC object or, rarely, a
/// JSON array of them. Unparseable bodies yield no messages (mirrors the stdio
/// transport's tolerance for non-JSON server output).
fn parse_json_messages(body: &str) -> Vec<Incoming> {
    let trimmed = body.trim();
    if let Ok(array) = serde_json::from_str::<Vec<Incoming>>(trimmed) {
        return array;
    }
    serde_json::from_str::<Incoming>(trimmed).map_or_else(|_| Vec::new(), |msg| vec![msg])
}

/// Parse an SSE body into JSON-RPC messages. Each event's concatenated `data:`
/// payload is decoded as one [`Incoming`]; non-`data` fields and comments are
/// ignored, and undecodable events are skipped.
fn parse_sse(body: &str) -> Vec<Incoming> {
    let mut out = Vec::new();
    let mut data = String::new();
    for line in body.lines() {
        if line.is_empty() {
            flush_event(&mut out, &mut data);
        } else if let Some(rest) = line.strip_prefix("data:") {
            // SSE permits an optional single space after the field colon.
            let chunk = rest.strip_prefix(' ').unwrap_or(rest);
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(chunk);
        }
    }
    // A final event may not be terminated by a blank line.
    flush_event(&mut out, &mut data);
    out
}

/// Decode the accumulated event `data` as one message (if it parses) and reset
/// the buffer for the next event.
fn flush_event(out: &mut Vec<Incoming>, data: &mut String) {
    if let Ok(msg) = serde_json::from_str::<Incoming>(data.trim()) {
        out.push(msg);
    }
    data.clear();
}

#[cfg(test)]
mod tests {
    use super::{parse_json_messages, parse_sse};

    #[test]
    fn json_single_object_parses() {
        let body = r#"{"jsonrpc":"2.0","id":4,"result":{"ok":true}}"#;
        let msgs = parse_json_messages(body);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs.first().and_then(|m| m.id), Some(4));
    }

    #[test]
    fn json_array_parses_all() {
        let body = r#"[{"jsonrpc":"2.0","id":1,"result":{}},{"jsonrpc":"2.0","id":2,"result":{}}]"#;
        let msgs = parse_json_messages(body);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs.get(1).and_then(|m| m.id), Some(2));
    }

    #[test]
    fn json_garbage_yields_nothing() {
        assert!(parse_json_messages("not json at all").is_empty());
        assert!(parse_json_messages("").is_empty());
    }

    #[test]
    fn sse_single_event_parses() {
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{}}\n\n";
        let msgs = parse_sse(body);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs.first().and_then(|m| m.id), Some(7));
    }

    #[test]
    fn sse_multiple_events_parse_in_order() {
        let body = "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\n\
                    data: {\"jsonrpc\":\"2.0\",\"id\":9,\"result\":{}}\n\n";
        let msgs = parse_sse(body);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs.first().and_then(|m| m.method.clone()), Some("notifications/progress".to_owned()));
        assert_eq!(msgs.get(1).and_then(|m| m.id), Some(9));
    }

    #[test]
    fn sse_multiline_data_is_concatenated() {
        // Two data lines for one event join with a newline into one JSON document.
        let body = "data: {\"jsonrpc\":\"2.0\",\n\
                    data: \"id\":3,\"result\":{}}\n\n";
        let msgs = parse_sse(body);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs.first().and_then(|m| m.id), Some(3));
    }

    #[test]
    fn sse_trailing_event_without_blank_line() {
        let body = "data: {\"jsonrpc\":\"2.0\",\"id\":5,\"result\":{}}";
        let msgs = parse_sse(body);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs.first().and_then(|m| m.id), Some(5));
    }
}
