//! SSE stream parsing for Claude Code API responses.

use std::io::{BufRead as _, BufReader};
use std::sync::mpsc::Sender;

use serde::Deserialize;
use serde_json::Value;

use crate::infra::tools::ToolUse;
use crate::llms::StreamEvent;
use crate::llms::error::LlmError;

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

/// Mutable state accumulated while consuming a Claude SSE stream.
#[derive(Default)]
struct SseState {
    /// Prompt (input) tokens reported by usage frames.
    input_tokens: usize,
    /// Completion (output) tokens reported by usage frames.
    output_tokens: usize,
    /// Prompt tokens served from cache.
    cache_hit_tokens: usize,
    /// Prompt tokens that missed the cache (fresh cache writes).
    cache_miss_tokens: usize,
    /// In-flight tool call: `(id, name, partial_input_json)`.
    current_tool: Option<(String, String, String)>,
    /// Normalized stop reason from the terminal `message_delta`.
    stop_reason: Option<String>,
}

/// Handle a `content_block_delta` event: stream text chunks or accumulate
/// partial tool-input JSON (emitting tool progress as it grows).
fn handle_block_delta(delta: StreamDelta, tx: &Sender<StreamEvent>, st: &mut SseState) {
    match delta.delta_type.as_deref() {
        Some("text_delta") => {
            if let Some(text) = delta.text {
                let _r = tx.send(StreamEvent::Chunk(text));
            }
        }
        Some("input_json_delta") => {
            if let Some(json) = delta.partial_json
                && let Some(tool) = st.current_tool.as_mut()
            {
                tool.2.push_str(&json);
                let _r = tx.send(StreamEvent::ToolProgress { name: tool.1.clone(), input_so_far: tool.2.clone() });
            }
        }
        _ => {}
    }
}

/// Fold a `message_start` usage frame (input + cache token counts) into `st`.
const fn handle_message_start(msg_body: StreamMessageBody, st: &mut SseState) {
    let Some(usage) = msg_body.usage else { return };
    if let Some(hit) = usage.cache_read {
        st.cache_hit_tokens = hit;
    }
    if let Some(miss) = usage.cache_creation {
        st.cache_miss_tokens = miss;
    }
    if let Some(inp) = usage.input {
        st.input_tokens = inp;
    }
}

/// Fold a `message_delta` event (stop reason + running token usage) into `st`.
fn handle_message_delta(event: &StreamMessage, st: &mut SseState) {
    if let Some(delta) = event.delta.as_ref()
        && let Some(reason) = delta.stop_reason.as_ref()
    {
        st.stop_reason = Some(reason.clone());
    }
    if let Some(usage) = event.usage.as_ref() {
        if let Some(inp) = usage.input {
            st.input_tokens = inp;
        }
        if let Some(out) = usage.output {
            st.output_tokens = out;
        }
    }
}

/// Dispatch one parsed SSE event. Returns `true` when the stream should stop
/// (`message_stop` or a logged `error` event).
fn handle_sse_event(event: StreamMessage, tx: &Sender<StreamEvent>, st: &mut SseState, ctx: &SseErrorCtx<'_>) -> bool {
    match event.event_type.as_str() {
        "content_block_start" => {
            if let Some(block) = event.content_block
                && block.block_type.as_deref() == Some("tool_use")
            {
                let name = block.name.unwrap_or_default();
                let _r = tx.send(StreamEvent::ToolProgress { name: name.clone(), input_so_far: String::new() });
                st.current_tool = Some((block.id.unwrap_or_default(), name, String::new()));
            }
            false
        }
        "content_block_delta" => {
            if let Some(delta) = event.delta {
                handle_block_delta(delta, tx, st);
            }
            false
        }
        "content_block_stop" => {
            if let Some((id, name, input_json)) = st.current_tool.take() {
                let input: Value =
                    serde_json::from_str(&input_json).unwrap_or_else(|_| Value::Object(serde_json::Map::new()));
                let _r = tx.send(StreamEvent::ToolUse(ToolUse::new(id, name, input)));
            }
            false
        }
        "message_start" => {
            if let Some(msg_body) = event.message {
                handle_message_start(msg_body, st);
            }
            false
        }
        "message_delta" => {
            handle_message_delta(&event, st);
            false
        }
        "message_stop" => true,
        "error" => {
            log_sse_error(ctx.json_str, ctx.total_bytes, ctx.line_count, ctx.last_lines);
            true
        }
        _ => false,
    }
}

/// Fields needed to log a raw SSE `error` event for post-mortem.
struct SseErrorCtx<'ctx> {
    /// Raw JSON payload of the error event.
    json_str: &'ctx str,
    /// Total bytes read from the stream so far.
    total_bytes: usize,
    /// Number of SSE lines read so far.
    line_count: usize,
    /// Last few SSE data lines, for context.
    last_lines: &'ctx [String],
}

/// Context for building the verbose SSE stream-read error message.
struct ReadErrorCtx<'ctx> {
    /// In-flight tool call at the time of failure, if any.
    current_tool: Option<&'ctx (String, String, String)>,
    /// Total bytes read from the stream so far.
    total_bytes: usize,
    /// Number of SSE lines read so far.
    line_count: usize,
    /// Raw HTTP response headers, for post-mortem.
    resp_headers: &'ctx str,
    /// Last few SSE data lines, for context.
    last_lines: &'ctx [String],
}

/// Build the verbose stream-read error string (error kind, root cause, position,
/// in-flight tool, response headers, last SSE lines).
fn build_read_error(e: &std::io::Error, ctx: &ReadErrorCtx<'_>) -> String {
    let error_kind = format!("{:?}", e.kind());
    let mut root_cause = String::new();
    let mut source: Option<&dyn std::error::Error> = std::error::Error::source(e);
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
         Error kind: {} | Root cause: {}\n\
         Stream position: {} bytes, {} lines read\n\
         {}\n\
         Response headers:\n{}\n\
         Last SSE lines:\n{}",
        e,
        error_kind,
        if root_cause.is_empty() { "(none)".to_owned() } else { root_cause },
        ctx.total_bytes,
        ctx.line_count,
        tool_ctx,
        ctx.resp_headers,
        recent
    )
}

/// Parse an SSE stream from a Claude API response, sending events to the channel.
/// Returns (`input_tokens`, `output_tokens`, `cache_hit_tokens`, `cache_miss_tokens`, `stop_reason`).
pub(crate) fn parse_sse_stream(
    response: reqwest::blocking::Response,
    resp_headers: &str,
    tx: &Sender<StreamEvent>,
) -> Result<SseStreamResult, LlmError> {
    let mut reader = BufReader::new(response);
    let mut st = SseState::default();
    let mut total_bytes: usize = 0;
    let mut line_count: usize = 0;
    let mut last_lines: Vec<String> = Vec::new();

    loop {
        let mut raw_line = String::new();
        match reader.read_line(&mut raw_line) {
            Ok(0) => break,
            Ok(n) => {
                total_bytes = total_bytes.saturating_add(n);
                line_count = line_count.saturating_add(1);
            }
            Err(e) => {
                let verbose = build_read_error(
                    &e,
                    &ReadErrorCtx {
                        current_tool: st.current_tool.as_ref(),
                        total_bytes,
                        line_count,
                        resp_headers,
                        last_lines: &last_lines,
                    },
                );
                return Err(LlmError::StreamRead(verbose));
            }
        }
        let line = raw_line.trim_end_matches('\n').trim_end_matches('\r');

        if !line.starts_with("data: ") {
            continue;
        }

        if last_lines.len() >= 5 {
            let _r = last_lines.remove(0);
        }
        last_lines.push(line.to_owned());

        let json_str = line.get(6..).unwrap_or("");
        if json_str == "[DONE]" {
            break;
        }

        if let Ok(event) = serde_json::from_str::<StreamMessage>(json_str) {
            let ctx = SseErrorCtx { json_str, total_bytes, line_count, last_lines: &last_lines };
            if handle_sse_event(event, tx, &mut st, &ctx) {
                break;
            }
        }
    }

    Ok((st.input_tokens, st.output_tokens, st.cache_hit_tokens, st.cache_miss_tokens, st.stop_reason))
}

/// Log an SSE error event to `.context-pilot/errors/` for post-mortem debugging.
/// Appends to `sse_errors.log` so multiple occurrences are visible.
fn log_sse_error(json_str: &str, total_bytes: usize, line_count: usize, last_lines: &[String]) {
    use std::io::Write as _;

    let dir = std::path::Path::new(".context-pilot").join("errors");
    let _r1 = std::fs::create_dir_all(&dir);
    let path = dir.join("sse_errors.log");

    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let recent = if last_lines.is_empty() { "(none)".to_owned() } else { last_lines.join("\n") };
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
