//! SSE stream parsing for Claude Code API responses.

use std::io::{BufRead as _, BufReader};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::infra::tools::ToolUse;
use crate::llms::StreamEvent;
use crate::llms::error::LlmError;

/// Maximum time to wait for the next SSE byte before treating the stream as a
/// silent application-level hold and aborting (→ retry on a fresh connection).
///
/// The Anthropic SSE stream is never legitimately silent this long: `ping`
/// events + token/thinking deltas flow continuously, and even time-to-first-byte
/// under load stays well under this. The pathological "silent-hold" (server
/// accepts the request, keeps the TCP socket alive, but never sends a byte) lasts
/// minutes — so 120s cleanly separates the two. A false trip self-heals via the
/// existing retry path; `MAX_API_RETRIES` bounds repeats. This is the definitive
/// fix for the freeze TCP keepalive could not catch (the peer is alive, just mute).
const IDLE_READ_TIMEOUT: Duration = Duration::from_secs(120);

/// Maximum time to wait for the HTTP **response headers** (i.e. for the blocking
/// `.send()` call to return) before treating the request as a silent hold and
/// aborting (→ retry on a fresh connection).
///
/// `connect_timeout` only bounds the TCP handshake; once connected, reqwest's
/// blocking `.send()` blocks awaiting the response status line + headers with no
/// timeout of its own (we deliberately set `.timeout(None)` so legit long streams
/// are never cut). The pathological silent-hold can occur HERE — the server
/// accepts the connection, keeps it TCP-alive, but never sends the response head.
/// [`IdleTimeoutReader`] only guards the body read that happens *after* `.send()`
/// returns, so this phase needs its own guard. 60s comfortably exceeds a normal
/// time-to-first-header (a few seconds, even under model queueing — the first SSE
/// *byte* may lag but headers do not) while aborting long before the agent wall.
const SEND_HEADER_TIMEOUT: Duration = Duration::from_secs(60);

/// Run a blocking `.send()` under a response-header timeout.
///
/// Moves the fully-configured `builder` onto a detached thread that performs the
/// blocking send, and waits up to [`SEND_HEADER_TIMEOUT`] for the response head.
/// On timeout, returns `StreamRead` so the request aborts and retries on a fresh
/// connection instead of riding the agent wall. Same leak caveat as
/// [`IdleTimeoutReader`]: a timed-out send thread parks on the dead socket until
/// the process exits (negligible for short-lived Terminal-Bench tasks).
pub(crate) fn send_with_header_timeout(
    builder: reqwest::blocking::RequestBuilder,
) -> Result<reqwest::blocking::Response, LlmError> {
    let (tx, rx) = std::sync::mpsc::channel();
    let _handle = std::thread::spawn(move || {
        let _r = tx.send(builder.send());
    });
    match rx.recv_timeout(SEND_HEADER_TIMEOUT) {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(e)) => Err(LlmError::Network(e.to_string())),
        Err(RecvTimeoutError::Timeout) => Err(LlmError::StreamRead(format!(
            "no response headers within {}s — aborting to retry on a fresh connection \
             (silent application-level hold before response head)",
            SEND_HEADER_TIMEOUT.as_secs()
        ))),
        Err(RecvTimeoutError::Disconnected) => {
            Err(LlmError::Network("send thread disconnected before responding".to_string()))
        }
    }
}

/// Idle-guarded line reader for a blocking SSE response.
///
/// `reqwest`'s **blocking** `ClientBuilder` exposes no `read_timeout` (only the
/// async builder does), so we cannot bound reads at the socket. Instead we move
/// the `Response` onto a dedicated reader thread that pushes one line at a time
/// over a channel, and the consumer waits with [`Receiver::recv_timeout`]. If no
/// line arrives within [`IDLE_READ_TIMEOUT`], `next_line` returns a `StreamRead`
/// error — bounding the hang instead of blocking forever.
///
/// Caveat: on a genuine silent-hold the reader thread stays parked on the dead
/// socket until the process exits. In Terminal-Bench (one short-lived process per
/// task) this is negligible; interactively a stall is rare. A future hardening
/// could switch to the async client's real `read_timeout`.
pub(crate) struct IdleTimeoutReader {
    /// Channel of read results: `Ok(Some(line))` per line, `Ok(None)` on EOF,
    /// `Err` on a read error. The sender lives on the reader thread.
    rx: Receiver<std::io::Result<Option<String>>>,
}

impl IdleTimeoutReader {
    /// Spawn the reader thread that owns `response` and streams its lines.
    pub(crate) fn spawn(response: reqwest::blocking::Response) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let _handle = std::thread::spawn(move || {
            let mut reader = BufReader::new(response);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        let _r = tx.send(Ok(None)); // EOF
                        break;
                    }
                    Ok(_) => {
                        if tx.send(Ok(Some(line))).is_err() {
                            break; // consumer gone (timed out + abandoned)
                        }
                    }
                    Err(e) => {
                        let _r = tx.send(Err(e));
                        break;
                    }
                }
            }
        });
        Self { rx }
    }

    /// Wait for the next line, bounded by [`IDLE_READ_TIMEOUT`].
    ///
    /// Returns `Ok(Some(line))` for a line, `Ok(None)` for EOF/disconnect, and
    /// `Err(LlmError::StreamRead)` for a read error or an idle timeout.
    pub(crate) fn next_line(&self) -> Result<Option<String>, LlmError> {
        match self.rx.recv_timeout(IDLE_READ_TIMEOUT) {
            Ok(Ok(opt)) => Ok(opt),
            Ok(Err(e)) => Err(LlmError::StreamRead(format!("SSE read error: {e}"))),
            Err(RecvTimeoutError::Timeout) => Err(LlmError::StreamRead(format!(
                "SSE stream idle for {}s with no data — aborting to retry on a fresh connection \
                 (silent application-level hold)",
                IDLE_READ_TIMEOUT.as_secs()
            ))),
            Err(RecvTimeoutError::Disconnected) => Ok(None),
        }
    }
}

/// Content block metadata from SSE stream events.
#[derive(Debug, Deserialize)]
pub(crate) struct StreamContentBlock {
    /// Block type (e.g. `text`, `tool_use`)
    #[serde(rename = "type")]
    pub block_type: Option<String>,
    /// Block ID (for `tool_use` blocks)
    pub id: Option<String>,
    /// Tool name (for `tool_use` blocks)
    pub name: Option<String>,
}

/// Delta payload from SSE stream events.
#[derive(Debug, Deserialize)]
pub(crate) struct StreamDelta {
    /// Delta type (e.g. `text_delta`, `input_json_delta`)
    #[serde(rename = "type")]
    pub delta_type: Option<String>,
    /// Text content delta
    pub text: Option<String>,
    /// Partial JSON for tool input
    pub partial_json: Option<String>,
    /// Stop reason (e.g. `end_turn`, `tool_use`)
    pub stop_reason: Option<String>,
}

/// Message body from `message_start` events.
#[derive(Debug, Deserialize)]
pub(crate) struct StreamMessageBody {
    /// Token usage statistics
    pub usage: Option<StreamUsage>,
}

/// Top-level SSE stream event from the Claude Code API.
#[derive(Debug, Deserialize)]
pub(crate) struct StreamMessage {
    /// Event type (e.g. `content_block_start`, `message_delta`)
    #[serde(rename = "type")]
    pub event_type: String,
    /// Content block metadata (for `block_start` events)
    pub content_block: Option<StreamContentBlock>,
    /// Delta payload (for delta events)
    pub delta: Option<StreamDelta>,
    /// Token usage statistics
    pub usage: Option<StreamUsage>,
    /// Message body (for `message_start` events)
    pub message: Option<StreamMessageBody>,
}

/// Token usage statistics from the Claude Code API.
#[derive(Debug, Deserialize)]
pub(crate) struct StreamUsage {
    /// Number of input tokens consumed
    #[serde(rename = "input_tokens")]
    pub input: Option<usize>,
    /// Number of output tokens generated
    #[serde(rename = "output_tokens")]
    pub output: Option<usize>,
    /// Number of tokens written to cache
    #[serde(rename = "cache_creation_input_tokens")]
    pub cache_creation: Option<usize>,
    /// Number of tokens read from cache
    #[serde(rename = "cache_read_input_tokens")]
    pub cache_read: Option<usize>,
}

/// Parsed SSE stream result: (`input_tokens`, `output_tokens`, `cache_hit`, `cache_miss`, `stop_reason`).
pub(crate) type SseStreamResult = (usize, usize, usize, usize, Option<String>);

/// Parse an SSE stream from a Claude API response, sending events to the channel.
/// Returns (`input_tokens`, `output_tokens`, `cache_hit_tokens`, `cache_miss_tokens`, `stop_reason`).
pub(crate) fn parse_sse_stream(
    response: reqwest::blocking::Response,
    resp_headers: &str,
    tx: &Sender<StreamEvent>,
) -> Result<SseStreamResult, LlmError> {
    let reader = IdleTimeoutReader::spawn(response);
    let mut input_tokens = 0;
    let mut output_tokens = 0;
    let mut cache_hit_tokens = 0;
    let mut cache_miss_tokens = 0;
    let mut current_tool: Option<(String, String, String)> = None;
    let mut stop_reason: Option<String> = None;
    let mut total_bytes: usize = 0;
    let mut line_count: usize = 0;
    let mut last_lines: Vec<String> = Vec::new();

    loop {
        // Idle-guarded read: an idle/silent-hold beyond IDLE_READ_TIMEOUT returns
        // Err(StreamRead) here, which propagates → StreamEvent::Error → retry.
        let line = match reader.next_line() {
            Ok(Some(l)) => {
                total_bytes = total_bytes.saturating_add(l.len());
                line_count = line_count.saturating_add(1);
                l
            }
            Ok(None) => break, // EOF / reader gone
            Err(e) => {
                let tool_ctx = match &current_tool {
                    Some((id, name, partial)) => {
                        format!("In-flight tool: {} (id={}), partial input: {} bytes", name, id, partial.len())
                    }
                    None => "No tool in progress".to_string(),
                };
                let recent = if last_lines.is_empty() { "(no lines read)".to_string() } else { last_lines.join("\n") };
                let verbose = format!(
                    "{e}\n\
                     Stream position: {total_bytes} bytes, {line_count} lines read\n\
                     {tool_ctx}\n\
                     Response headers:\n{resp_headers}\n\
                     Last SSE lines:\n{recent}"
                );
                return Err(LlmError::StreamRead(verbose));
            }
        };
        let line = line.trim_end_matches('\n').trim_end_matches('\r');

        if !line.starts_with("data: ") {
            continue;
        }

        if last_lines.len() >= 5 {
            let _r = last_lines.remove(0);
        }
        last_lines.push(line.to_string());

        let json_str = line.get(6..).unwrap_or("");
        if json_str == "[DONE]" {
            break;
        }

        if let Ok(event) = serde_json::from_str::<StreamMessage>(json_str) {
            match event.event_type.as_str() {
                "content_block_start" => {
                    if let Some(block) = event.content_block
                        && block.block_type.as_deref() == Some("tool_use")
                    {
                        let name = block.name.unwrap_or_default();
                        let _r = tx.send(StreamEvent::ToolProgress { name: name.clone(), input_so_far: String::new() });
                        current_tool = Some((block.id.unwrap_or_default(), name, String::new()));
                    }
                }
                "content_block_delta" => {
                    if let Some(delta) = event.delta {
                        match delta.delta_type.as_deref() {
                            Some("text_delta") => {
                                if let Some(text) = delta.text {
                                    let _r = tx.send(StreamEvent::Chunk(text));
                                }
                            }
                            Some("input_json_delta") => {
                                if let Some(json) = delta.partial_json
                                    && let Some((_, ref name, ref mut input)) = current_tool
                                {
                                    input.push_str(&json);
                                    let _r = tx.send(StreamEvent::ToolProgress {
                                        name: name.clone(),
                                        input_so_far: input.clone(),
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }
                "content_block_stop" => {
                    if let Some((id, name, input_json)) = current_tool.take() {
                        let input: Value =
                            serde_json::from_str(&input_json).unwrap_or_else(|_| Value::Object(serde_json::Map::new()));
                        let _r = tx.send(StreamEvent::ToolUse(ToolUse { id, name, input }));
                    }
                }
                "message_start" => {
                    if let Some(msg_body) = event.message
                        && let Some(usage) = msg_body.usage
                    {
                        if let Some(hit) = usage.cache_read {
                            cache_hit_tokens = hit;
                        }
                        if let Some(miss) = usage.cache_creation {
                            cache_miss_tokens = miss;
                        }
                        if let Some(inp) = usage.input {
                            input_tokens = inp;
                        }
                    }
                }
                "message_delta" => {
                    if let Some(ref delta) = event.delta
                        && let Some(ref reason) = delta.stop_reason
                    {
                        stop_reason = Some(reason.clone());
                    }
                    if let Some(usage) = event.usage {
                        if let Some(inp) = usage.input {
                            input_tokens = inp;
                        }
                        if let Some(out) = usage.output {
                            output_tokens = out;
                        }
                    }
                }
                "message_stop" => break,
                "error" => {
                    // Log the raw SSE error event to disk for debugging.
                    // Don't alter the return flow — caller still gets Ok(...)
                    // so StreamEvent::Done fires as before, but now we have a trace.
                    log_sse_error(json_str, total_bytes, line_count, &last_lines);
                    break;
                }
                _ => {}
            }
        }
    }

    Ok((input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, stop_reason))
}

/// Log an SSE error event to `.context-pilot/errors/` for post-mortem debugging.
/// Appends to `sse_errors.log` so multiple occurrences are visible.
fn log_sse_error(json_str: &str, total_bytes: usize, line_count: usize, last_lines: &[String]) {
    use std::io::Write as _;

    let dir = std::path::Path::new(".context-pilot").join("errors");
    let _r1 = std::fs::create_dir_all(&dir);
    let path = dir.join("sse_errors.log");

    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let recent = if last_lines.is_empty() { "(none)".to_string() } else { last_lines.join("\n") };
    let entry = format!(
        "[{ts}] SSE error event (claude_code_api_key)\n\
         Stream position: {total_bytes} bytes, {line_count} lines\n\
         Error data: {json_str}\n\
         Last SSE lines:\n{recent}\n\
         ---\n"
    );

    let _r2 = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(entry.as_bytes()));
}
