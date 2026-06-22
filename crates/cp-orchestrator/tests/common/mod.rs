//! Shared test helpers for the transport integration suites: a minimal,
//! dependency-free blocking HTTP/1.1 client and an SSE reader, both over a raw
//! [`TcpStream`]. Hand-rolled on purpose — the point of these suites is to
//! exercise the real `tiny_http` server on the wire, and a raw client proves
//! the bytes round-trip without importing an async HTTP stack.
//!
//! Not a test target itself (a `tests/` subdirectory is a module, never an
//! auto-run integration binary), so it carries no `#[test]` functions.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::{Duration, Instant};

/// A parsed HTTP response: status code and body text.
pub(crate) struct HttpResponse {
    /// Numeric status (e.g. `200`, `404`).
    pub(crate) status: u16,
    /// Response body, decoded lossily as UTF-8.
    pub(crate) body: String,
}

/// Perform one blocking HTTP request, reading the full response to EOF.
///
/// Sends `Connection: close` so `tiny_http` closes the socket after the
/// response and `read_to_end` terminates. `headers` are extra request headers;
/// `body` (when `Some`) is sent with a matching `Content-Length`.
pub(crate) fn request(
    addr: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> HttpResponse {
    let mut stream = TcpStream::connect(addr).expect("connect");
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n");
    for (name, value) in headers {
        req.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(payload) = body {
        req.push_str(&format!("Content-Length: {}\r\n", payload.len()));
    }
    req.push_str("\r\n");

    stream.write_all(req.as_bytes()).expect("write request");
    if let Some(payload) = body {
        stream.write_all(payload).expect("write body");
    }
    stream.flush().expect("flush");

    let mut raw = Vec::new();
    let _read = stream.read_to_end(&mut raw).expect("read response");
    parse_response(&raw)
}

/// `GET` convenience wrapper.
pub(crate) fn get(addr: &str, path: &str, headers: &[(&str, &str)]) -> HttpResponse {
    request(addr, "GET", path, headers, None)
}

/// `POST` convenience wrapper with a JSON body.
pub(crate) fn post_json(addr: &str, path: &str, body: &[u8]) -> HttpResponse {
    request(addr, "POST", path, &[("Content-Type", "application/json")], Some(body))
}

/// Split a raw HTTP response into its status code and body.
fn parse_response(raw: &[u8]) -> HttpResponse {
    let text = String::from_utf8_lossy(raw);
    let status = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let body = text.split_once("\r\n\r\n").map_or(String::new(), |(_head, b)| b.to_owned());
    HttpResponse { status, body }
}

/// One parsed Server-Sent Event.
#[derive(Debug, Clone)]
pub(crate) struct SseEvent {
    /// The `id:` field, parsed as a `rev` when present.
    pub(crate) id: Option<u64>,
    /// The `event:` name.
    pub(crate) event: String,
    /// The concatenated `data:` payload.
    pub(crate) data: String,
}

/// Open an SSE stream and collect events until `want` are seen or `deadline`
/// elapses. Returns the parsed status line and the events gathered.
///
/// Comment/keep-alive lines (`:`-prefixed) are ignored. The socket is left to
/// drop (closing the stream) when the returned value goes out of scope.
pub(crate) fn sse_collect(
    addr: &str,
    path: &str,
    headers: &[(&str, &str)],
    want: usize,
    deadline: Duration,
) -> (u16, Vec<SseEvent>) {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream.set_read_timeout(Some(Duration::from_millis(200))).expect("set read timeout");

    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n", method = "GET");
    req.push_str("Accept: text/event-stream\r\n");
    for (name, value) in headers {
        req.push_str(&format!("{name}: {value}\r\n"));
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes()).expect("write request");
    stream.flush().expect("flush");

    let started = Instant::now();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let mut status: u16 = 0;
    let mut header_end: Option<usize> = None;
    let mut events = Vec::new();
    let mut parsed_upto = 0usize;

    while started.elapsed() < deadline && events.len() < want {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if let Some(got) = chunk.get(..n) {
                    buf.extend_from_slice(got);
                }
            }
            Err(_) => continue, // read timeout — re-check the deadline.
        }

        let text = String::from_utf8_lossy(&buf).into_owned();
        if header_end.is_none() {
            if let Some(idx) = text.find("\r\n\r\n") {
                status = parse_status_line(&text);
                header_end = Some(idx.saturating_add(4));
                parsed_upto = idx.saturating_add(4);
            } else {
                continue;
            }
        }

        // Parse complete event blocks (terminated by a blank line) from the
        // body region we have not yet consumed.
        let body = text.get(parsed_upto..).unwrap_or("");
        let mut consumed = 0usize;
        while let Some(rel) = find_block_end(&body[consumed..]) {
            let block = &body[consumed..consumed + rel];
            if let Some(event) = parse_event(block) {
                events.push(event);
            }
            consumed += rel;
        }
        parsed_upto = parsed_upto.saturating_add(consumed);
    }

    (status, events)
}

/// Find the end (exclusive, past the terminator) of the first event block in
/// `s`, where blocks are separated by a blank line (`\n\n`).
fn find_block_end(s: &str) -> Option<usize> {
    s.find("\n\n").map(|i| i.saturating_add(2))
}

/// Parse a single SSE event block into an [`SseEvent`], or `None` if it carries
/// no `event:`/`data:` lines (e.g. a pure comment block).
fn parse_event(block: &str) -> Option<SseEvent> {
    let mut id = None;
    let mut event = String::new();
    let mut data = String::new();
    let mut saw_field = false;
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("id:") {
            id = rest.trim().parse().ok();
            saw_field = true;
        } else if let Some(rest) = line.strip_prefix("event:") {
            event = rest.trim().to_owned();
            saw_field = true;
        } else if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.trim());
            saw_field = true;
        }
    }
    saw_field.then_some(SseEvent { id, event, data })
}

/// Parse the status code out of an HTTP status line at the start of `text`.
fn parse_status_line(text: &str) -> u16 {
    text.lines().next().and_then(|line| line.split_whitespace().nth(1)).and_then(|code| code.parse().ok()).unwrap_or(0)
}
