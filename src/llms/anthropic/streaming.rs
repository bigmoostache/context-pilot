//! Shared SSE stream consumer for Anthropic-protocol providers.
//!
//! Extracted from `anthropic/mod.rs` and reused by `MiniMax` (identical wire
//! format) to keep both `stream()` bodies under the cognitive-complexity budget.
//! The event loop and its per-event handlers live here; each provider's
//! `stream()` only builds the request and forwards the parsed `Done` totals.

use std::io::{BufRead as _, BufReader};
use std::sync::mpsc::Sender;

use serde::Deserialize;
use serde_json::Value;

use crate::infra::tools::ToolUse;
use crate::llms::StreamEvent;
use crate::llms::error::LlmError;

/// Content block metadata from SSE stream events.
#[derive(Debug, Deserialize)]
pub(in crate::llms) struct StreamContentBlock {
    /// Block type (e.g. `text`, `tool_use`).
    #[serde(rename = "type")]
    pub block_type: Option<String>,
    /// Block ID (for `tool_use` blocks).
    pub id: Option<String>,
    /// Tool name (for `tool_use` blocks).
    pub name: Option<String>,
}

/// Delta payload from SSE stream events.
#[derive(Debug, Deserialize)]
pub(in crate::llms) struct StreamDelta {
    /// Delta type (e.g. `text_delta`, `input_json_delta`).
    #[serde(rename = "type")]
    pub delta_type: Option<String>,
    /// Text content delta.
    pub text: Option<String>,
    /// Partial JSON for tool input.
    pub partial_json: Option<String>,
    /// Stop reason (e.g. `end_turn`, `tool_use`).
    pub stop_reason: Option<String>,
}

/// Top-level SSE stream event from the Anthropic protocol.
#[derive(Debug, Deserialize)]
pub(in crate::llms) struct StreamMessage {
    /// Event type (e.g. `content_block_start`, `message_delta`).
    #[serde(rename = "type")]
    pub event_type: String,
    /// Content block metadata (for `block_start` events).
    pub content_block: Option<StreamContentBlock>,
    /// Delta payload (for delta events).
    pub delta: Option<StreamDelta>,
    /// Token usage statistics.
    pub usage: Option<StreamUsage>,
}

/// Token usage statistics from the Anthropic protocol.
#[derive(Debug, Deserialize)]
pub(in crate::llms) struct StreamUsage {
    /// Number of input tokens consumed.
    pub input_tokens: Option<usize>,
    /// Number of output tokens generated.
    pub output_tokens: Option<usize>,
}

/// Token/stop-reason totals accumulated while consuming the stream.
#[derive(Default)]
pub(in crate::llms) struct AnthTotals {
    /// Prompt (input) tokens reported by the terminal usage frame.
    pub input_tokens: usize,
    /// Completion (output) tokens reported by the terminal usage frame.
    pub output_tokens: usize,
    /// Normalized stop reason from the terminal `message_delta`.
    pub stop_reason: Option<String>,
    /// In-flight tool call: `(id, name, partial_input_json)`.
    pub current_tool: Option<(String, String, String)>,
}

/// Handle a `content_block_delta` event: stream text chunks or accumulate
/// partial tool-input JSON (emitting tool progress as it grows).
fn handle_block_delta(delta: StreamDelta, tx: &Sender<StreamEvent>, st: &mut AnthTotals) {
    match delta.delta_type.as_deref() {
        Some("text_delta") => {
            if let Some(text) = delta.text {
                let _r = tx.send(StreamEvent::Chunk(text));
            }
        }
        Some("input_json_delta") => {
            if let Some(json) = delta.partial_json
                && let Some((_, name, input)) = st.current_tool.as_mut()
            {
                input.push_str(&json);
                let _r = tx.send(StreamEvent::ToolProgress { name: name.clone(), input_so_far: input.clone() });
            }
        }
        _ => {}
    }
}

/// Fold a `message_delta` event (stop reason + terminal token usage) into `st`.
fn handle_message_delta(event: &StreamMessage, st: &mut AnthTotals) {
    if let Some(delta) = &event.delta
        && let Some(reason) = &delta.stop_reason
    {
        st.stop_reason = Some(reason.clone());
    }
    if let Some(usage) = &event.usage {
        if let Some(inp) = usage.input_tokens {
            st.input_tokens = inp;
        }
        if let Some(out) = usage.output_tokens {
            st.output_tokens = out;
        }
    }
}

/// Fields needed to log a raw SSE `error` event for post-mortem.
struct SseErrorCtx<'ctx> {
    /// Provider label for the error log.
    provider: &'static str,
    /// Raw JSON payload of the error event.
    json_str: &'ctx str,
    /// Total bytes read from the stream so far.
    total_bytes: usize,
    /// Number of SSE lines read so far.
    line_count: usize,
    /// Last few SSE data lines, for context.
    last_lines: &'ctx [String],
}

/// Dispatch one parsed SSE event. Returns `true` when the stream should stop
/// (`message_stop` or a logged `error` event).
fn handle_sse_event(
    event: StreamMessage,
    tx: &Sender<StreamEvent>,
    st: &mut AnthTotals,
    ctx: &SseErrorCtx<'_>,
) -> bool {
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
        "message_delta" => {
            handle_message_delta(&event, st);
            false
        }
        "message_stop" => true,
        "error" => {
            crate::llms::log_sse_error(&crate::llms::SseErrorContext {
                provider: ctx.provider,
                json_str: ctx.json_str,
                total_bytes: ctx.total_bytes,
                line_count: ctx.line_count,
                last_lines: ctx.last_lines,
            });
            true
        }
        _ => false,
    }
}

/// Context for building an SSE stream-read error message.
struct ReadErrorCtx<'ctx> {
    /// In-flight tool call at the time of failure, if any.
    current_tool: Option<&'ctx (String, String, String)>,
    /// Total bytes read from the stream so far.
    total_bytes: usize,
    /// Number of SSE lines read so far.
    line_count: usize,
    /// Last few SSE data lines, for context.
    last_lines: &'ctx [String],
}

/// Build the stream-read error string (position, in-flight tool, last SSE lines).
fn build_read_error(e: &std::io::Error, ctx: &ReadErrorCtx<'_>) -> String {
    let tool_ctx = ctx.current_tool.map_or_else(
        || "No tool in progress".to_owned(),
        |(id, name, partial)| format!("In-flight tool: {} (id={}), partial: {} bytes", name, id, partial.len()),
    );
    let recent = if ctx.last_lines.is_empty() { "(no lines read)".to_owned() } else { ctx.last_lines.join("\n") };
    format!(
        "{e}\nStream position: {} bytes, {} lines read\n{tool_ctx}\nLast SSE lines:\n{recent}",
        ctx.total_bytes, ctx.line_count
    )
}

/// Read an Anthropic-protocol SSE response to completion, streaming chunks and
/// tool events to `tx`. Returns accumulated token/stop-reason totals; the caller
/// sends the terminal `StreamEvent::Done`. `provider` labels error logs.
pub(in crate::llms) fn consume_anthropic_stream(
    response: reqwest::blocking::Response,
    tx: &Sender<StreamEvent>,
    provider: &'static str,
) -> Result<AnthTotals, LlmError> {
    let mut reader = BufReader::new(response);
    let mut st = AnthTotals::default();
    let mut total_bytes: usize = 0;
    let mut line_count: usize = 0;
    let mut last_lines: Vec<String> = Vec::new();

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
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
                        last_lines: &last_lines,
                    },
                );
                return Err(LlmError::StreamRead(verbose));
            }
        }
        let line = line.trim_end_matches('\n').trim_end_matches('\r');

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
            let ctx = SseErrorCtx { provider, json_str, total_bytes, line_count, last_lines: &last_lines };
            if handle_sse_event(event, tx, &mut st, &ctx) {
                break;
            }
        }
    }

    Ok(st)
}
