//! SSE stream parsing for Claude Code API responses.

use std::io::{BufRead, BufReader};
use std::sync::mpsc::Sender;

use serde::Deserialize;
use serde_json::Value;

use crate::infra::tools::ToolUse;
use crate::llms::StreamEvent;
use crate::llms::error::LlmError;

#[derive(Debug, Deserialize)]
pub(super) struct StreamContentBlock {
    #[serde(rename = "type")]
    pub block_type: Option<String>,
    pub id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct StreamDelta {
    #[serde(rename = "type")]
    pub delta_type: Option<String>,
    pub text: Option<String>,
    pub partial_json: Option<String>,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct StreamMessageBody {
    pub usage: Option<StreamUsage>,
}

#[derive(Debug, Deserialize)]
pub(super) struct StreamMessage {
    #[serde(rename = "type")]
    pub event_type: String,
    pub content_block: Option<StreamContentBlock>,
    pub delta: Option<StreamDelta>,
    pub usage: Option<StreamUsage>,
    pub message: Option<StreamMessageBody>,
}

#[derive(Debug, Deserialize)]
#[expect(clippy::struct_field_names, reason = "Field names mirror the Anthropic API response")]
pub(super) struct StreamUsage {
    pub input_tokens: Option<usize>,
    pub output_tokens: Option<usize>,
    pub cache_creation_input_tokens: Option<usize>,
    pub cache_read_input_tokens: Option<usize>,
}

/// Parse an SSE stream from a Claude API response, sending events to the channel.
/// Returns (`input_tokens`, `output_tokens`, `cache_hit_tokens`, `cache_miss_tokens`, `stop_reason`).
pub(super) fn parse_sse_stream(
    response: reqwest::blocking::Response,
    resp_headers: &str,
    tx: &Sender<StreamEvent>,
) -> Result<(usize, usize, usize, usize, Option<String>), LlmError> {
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
                total_bytes += n;
                line_count += 1;
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

        let json_str = &line[6..];
        if json_str == "[DONE]" {
            break;
        }

        if let Ok(event) = serde_json::from_str::<StreamMessage>(json_str) {
            match event.event_type.as_str() {
                "content_block_start" => {
                    if let Some(block) = event.content_block
                        && block.block_type.as_deref() == Some("tool_use")
                    {
                        current_tool =
                            Some((block.id.unwrap_or_default(), block.name.unwrap_or_default(), String::new()));
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
                                    && let Some((_, _, ref mut input)) = current_tool
                                {
                                    input.push_str(&json);
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
                        if let Some(hit) = usage.cache_read_input_tokens {
                            cache_hit_tokens = hit;
                        }
                        if let Some(miss) = usage.cache_creation_input_tokens {
                            cache_miss_tokens = miss;
                        }
                        if let Some(inp) = usage.input_tokens {
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
                        if let Some(inp) = usage.input_tokens {
                            input_tokens = inp;
                        }
                        if let Some(out) = usage.output_tokens {
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
    use std::io::Write;

    let dir = std::path::Path::new(".context-pilot").join("errors");
    let _r = std::fs::create_dir_all(&dir);
    let path = dir.join("sse_errors.log");

    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let recent = if last_lines.is_empty() { "(none)".to_string() } else { last_lines.join("\n") };
    let entry = format!(
        "[{ts}] SSE error event (claude_code_api_key)\n\
         Stream position: {total_bytes} bytes, {line_count} lines\n\
         Error data: {json_str}\n\
         Last SSE lines:\n{recent}\n\
         ---\n"
    );

    let _r = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(entry.as_bytes()));
}
