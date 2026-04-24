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
pub(super) struct StreamContentBlock {
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
pub(super) struct StreamDelta {
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
pub(super) struct StreamMessageBody {
    /// Token usage statistics
    pub usage: Option<StreamUsage>,
}

/// Top-level SSE stream event from the Claude Code API.
#[derive(Debug, Deserialize)]
pub(super) struct StreamMessage {
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
pub(super) struct StreamUsage {
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
pub(super) type SseStreamResult = (usize, usize, usize, usize, Option<String>);

/// Parse an SSE stream from a Claude API response, sending events to the channel.
/// Returns (`input_tokens`, `output_tokens`, `cache_hit_tokens`, `cache_miss_tokens`, `stop_reason`).
pub(super) fn parse_sse_stream(
    response: reqwest::blocking::Response,
    resp_headers: &str,
    tx: &Sender<StreamEvent>,
) -> Result<SseStreamResult, LlmError> {
    let mut reader = BufReader::new(response);
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
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(n) => {
                total_bytes = total_bytes.saturating_add(n);
                line_count = line_count.saturating_add(1);
            }
            Err(e) => {
                let error_kind = format!("{:?}", e.kind());
                let mut root_cause = String::new();
                let mut source: Option<&dyn std::error::Error> = std::error::Error::source(&e);
                while let Some(s) = source {
                    root_cause = format!("{s}");
                    source = std::error::Error::source(s);
                }
                let tool_ctx = match &current_tool {
                    Some((id, name, partial)) => {
                        format!("In-flight tool: {} (id={}), partial input: {} bytes", name, id, partial.len())
                    }
                    None => "No tool in progress".to_string(),
                };
                let recent = if last_lines.is_empty() { "(no lines read)".to_string() } else { last_lines.join("\n") };
                let verbose = format!(
                    "{}\n\
                     Error kind: {} | Root cause: {}\n\
                     Stream position: {} bytes, {} lines read\n\
                     {}\n\
                     Response headers:\n{}\n\
                     Last SSE lines:\n{}",
                    e,
                    error_kind,
                    if root_cause.is_empty() { "(none)".to_string() } else { root_cause },
                    total_bytes,
                    line_count,
                    tool_ctx,
                    resp_headers,
                    recent
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
