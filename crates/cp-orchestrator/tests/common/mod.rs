//! Shared test helpers for the transport integration suites: a minimal,
//! dependency-free blocking HTTP/1.1 client and an SSE reader, both over a raw
//! [`TcpStream`]. Hand-rolled on purpose — the point of these suites is to
//! exercise the real `tiny_http` server on the wire, and a raw client proves
//! the bytes round-trip without importing an async HTTP stack.
//!
//! Not a test target itself (a `tests/` subdirectory is a module, never an
//! auto-run integration binary), so it carries no `#[test]` functions. The
//! helpers live in a `#[cfg(test)] mod` so clippy's `allow-*-in-tests`
//! relaxations reach the `expect()` calls that assert the wire round-trip, and
//! are re-exported at the module root so sibling suites keep calling
//! `common::get(…)` / `common::sse_collect(…)` unchanged.

#[cfg(test)]
pub(crate) use inner::*;

#[cfg(test)]
mod inner {
    use std::fmt::Write as _;
    use std::io::{Read as _, Write as _};
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    /// A parsed HTTP response: status code and body text.
    pub(crate) struct HttpResponse {
        /// Numeric status (e.g. `200`, `404`).
        pub status: u16,
        /// Response body, decoded lossily as UTF-8.
        pub body: String,
    }

    /// The variable parts of a single HTTP request, bundled so [`request`]
    /// stays within the argument-count cap.
    pub(crate) struct ReqSpec<'spec> {
        /// HTTP method (`GET`, `POST`, …).
        pub method: &'spec str,
        /// Request path (e.g. `/api/fleet`).
        pub path: &'spec str,
        /// Extra request headers.
        pub headers: &'spec [(&'spec str, &'spec str)],
        /// Optional body; sent with a matching `Content-Length`.
        pub body: Option<&'spec [u8]>,
    }

    /// Perform one blocking HTTP request, reading the full response to EOF.
    ///
    /// Sends `Connection: close` so `tiny_http` closes the socket after the
    /// response and `read_to_end` terminates.
    pub(crate) fn request(addr: &str, spec: &ReqSpec<'_>) -> HttpResponse {
        let mut stream = TcpStream::connect(addr).expect("connect");
        let mut req = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n",
            method = spec.method,
            path = spec.path
        );
        for &(name, value) in spec.headers {
            _ = write!(req, "{name}: {value}\r\n");
        }
        if let Some(payload) = spec.body {
            _ = write!(req, "Content-Length: {}\r\n", payload.len());
        }
        req.push_str("\r\n");

        stream.write_all(req.as_bytes()).expect("write request");
        if let Some(payload) = spec.body {
            stream.write_all(payload).expect("write body");
        }
        stream.flush().expect("flush");

        let mut raw = Vec::new();
        let _read = stream.read_to_end(&mut raw).expect("read response");
        parse_response(&raw)
    }

    /// `GET` convenience wrapper.
    pub(crate) fn get(addr: &str, path: &str, headers: &[(&str, &str)]) -> HttpResponse {
        request(addr, &ReqSpec { method: "GET", path, headers, body: None })
    }

    /// `POST` convenience wrapper with a JSON body.
    pub(crate) fn post_json(addr: &str, path: &str, body: &[u8]) -> HttpResponse {
        request(
            addr,
            &ReqSpec { method: "POST", path, headers: &[("Content-Type", "application/json")], body: Some(body) },
        )
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
        pub id: Option<u64>,
        /// The `event:` name.
        pub event: String,
        /// The concatenated `data:` payload.
        pub data: String,
    }

    /// How long to wait for how many SSE events, bundled so [`sse_collect`]
    /// stays within the argument-count cap.
    #[derive(Clone, Copy)]
    pub(crate) struct SseWait {
        /// Stop once this many events are collected.
        pub want: usize,
        /// Give up after this much wall-clock time.
        pub deadline: Duration,
    }

    /// Open an SSE stream and collect events until `wait.want` are seen or
    /// `wait.deadline` elapses. Returns the parsed status line and the events
    /// gathered.
    ///
    /// Comment/keep-alive lines (`:`-prefixed) are ignored. The socket is left
    /// to drop (closing the stream) when the returned value goes out of scope.
    pub(crate) fn sse_collect(addr: &str, path: &str, headers: &[(&str, &str)], wait: SseWait) -> (u16, Vec<SseEvent>) {
        let SseWait { want, deadline } = wait;
        let mut stream = TcpStream::connect(addr).expect("connect");
        stream.set_read_timeout(Some(Duration::from_millis(200))).expect("set read timeout");

        let mut req = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n", method = "GET");
        req.push_str("Accept: text/event-stream\r\n");
        for &(name, value) in headers {
            _ = write!(req, "{name}: {value}\r\n");
        }
        req.push_str("\r\n");
        stream.write_all(req.as_bytes()).expect("write request");
        stream.flush().expect("flush");

        let started = Instant::now();
        let mut buf = Vec::new();
        let mut chunk = vec![0u8; 1024];
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
                let Some(idx) = text.find("\r\n\r\n") else {
                    continue;
                };
                status = parse_status_line(&text);
                let end = idx.saturating_add(4);
                header_end = Some(end);
                parsed_upto = end;
            }

            // Parse complete event blocks (terminated by a blank line) from the
            // body region we have not yet consumed.
            let body = text.get(parsed_upto..).unwrap_or("");
            let consumed = drain_events(body, &mut events);
            parsed_upto = parsed_upto.saturating_add(consumed);
        }

        (status, events)
    }

    /// Drain every complete event block from `body` into `events`, returning
    /// how many bytes were consumed (the offset past the last block parsed).
    fn drain_events(body: &str, events: &mut Vec<SseEvent>) -> usize {
        let mut consumed = 0usize;
        while let Some(rel) = find_block_end(body.get(consumed..).unwrap_or("")) {
            let end = consumed.saturating_add(rel);
            if let Some(block) = body.get(consumed..end)
                && let Some(event) = parse_event(block)
            {
                events.push(event);
            }
            consumed = end;
        }
        consumed
    }

    /// Find the end (exclusive, past the terminator) of the first event block in
    /// `s`, where blocks are separated by a blank line (`\n\n`).
    fn find_block_end(s: &str) -> Option<usize> {
        s.find("\n\n").map(|i| i.saturating_add(2))
    }

    /// Parse a single SSE event block into an [`SseEvent`], or `None` if it
    /// carries no `event:`/`data:` lines (e.g. a pure comment block).
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
                rest.trim().clone_into(&mut event);
                saw_field = true;
            } else if let Some(rest) = line.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(rest.trim());
                saw_field = true;
            } else {
                // A comment line (`:`-prefixed keep-alive) or an unknown field —
                // ignored, contributing nothing to the parsed event.
            }
        }
        saw_field.then_some(SseEvent { id, event, data })
    }

    /// Parse the status code out of an HTTP status line at the start of `text`.
    fn parse_status_line(text: &str) -> u16 {
        text.lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse().ok())
            .unwrap_or(0)
    }
}
