//! SSE stream consumption for the Claude Code OAuth API.
//!
//! Extracted from `mod.rs` to keep `do_stream` under the cognitive-complexity
//! budget: the event loop and its per-event handlers live here, `do_stream`
//! only builds the request and forwards the parsed `Done` totals.

use std::io::{BufRead as _, BufReader};
use std::sync::mpsc::Sender;

use serde_json::Value;

use super::StreamMessage;
use super::debug;
use crate::infra::tools::ToolUse;
use crate::llms::StreamEvent;
use crate::llms::error::LlmError;

/// Token/stop-reason totals accumulated while consuming a Claude Code SSE stream.
#[derive(Default)]
pub(super) struct CcStreamTotals {
    /// Prompt (input) tokens reported by usage frames.
    pub input_tokens: usize,
    /// Completion (output) tokens reported by the final usage frame.
    pub output_tokens: usize,
    /// Prompt tokens served from cache.
    pub cache_hit_tokens: usize,
    /// Prompt tokens that missed the cache (fresh cache writes).
    pub cache_miss_tokens: usize,
    /// Normalized stop reason from the terminal `message_delta`.
    pub stop_reason: Option<String>,
    /// In-flight tool call: `(id, name, partial_input_json)`.
    pub current_tool: Option<(String, String, String)>,
}

/// Handle a `content_block_delta` event: stream text chunks or accumulate
/// partial tool-input JSON (emitting tool progress as it grows).
fn handle_block_delta(delta: super::stream_types::StreamDelta, tx: &Sender<StreamEvent>, st: &mut CcStreamTotals) {
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

/// Fold a `message_start` usage frame (input + cache token counts) into `st`.
fn handle_message_start(event: StreamMessage, st: &mut CcStreamTotals) {
    let Some(msg_body) = event.message else { return };
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
fn handle_message_delta(event: &StreamMessage, st: &mut CcStreamTotals) {
    if let Some(delta) = &event.delta
        && let Some(reason) = &delta.stop_reason
    {
        st.stop_reason = Some(reason.clone());
    }
    if let Some(usage) = &event.usage {
        if let Some(inp) = usage.input {
            st.input_tokens = inp;
        }
        if let Some(out) = usage.output {
            st.output_tokens = out;
        }
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

/// Dispatch one parsed SSE event. Returns `true` when the stream should stop
/// (`message_stop` or a logged `error` event).
fn handle_sse_event(
    event: StreamMessage,
    tx: &Sender<StreamEvent>,
    st: &mut CcStreamTotals,
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
                let _r = tx.send(StreamEvent::ToolUse(ToolUse { id, name, input }));
            }
            false
        }
        "message_start" => {
            handle_message_start(event, st);
            false
        }
        "message_delta" => {
            handle_message_delta(&event, st);
            false
        }
        "message_stop" => true,
        "error" => {
            crate::llms::log_sse_error(&crate::llms::SseErrorContext {
                provider: "claude_code",
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

/// Read a Claude Code SSE response to completion, streaming chunks and tool
/// events to `tx`. Returns accumulated token/stop-reason totals; the caller
/// sends the terminal `StreamEvent::Done` from them.
pub(super) fn consume_cc_stream(
    response: reqwest::blocking::Response,
    resp_headers: &str,
    tx: &Sender<StreamEvent>,
) -> Result<CcStreamTotals, LlmError> {
    let mut reader = BufReader::new(response);
    let mut st = CcStreamTotals::default();
    let mut total_bytes: usize = 0;
    let mut line_count: usize = 0;
    let mut last_lines: Vec<String> = Vec::new();

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(n) => {
                total_bytes = total_bytes.saturating_add(n);
                line_count = line_count.saturating_add(1);
            }
            Err(e) => {
                let verbose = debug::build_stream_read_error(&debug::StreamErrorContext {
                    err: &e,
                    current_tool: st.current_tool.as_ref(),
                    total_bytes,
                    line_count,
                    resp_headers,
                    last_lines: &last_lines,
                });
                return Err(LlmError::StreamRead(verbose));
            }
        }
        let line = line.trim_end_matches('\n').trim_end_matches('\r');

        if !line.starts_with("data: ") {
            continue;
        }

        // Keep last 5 data lines for error context
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

    Ok(st)
}
