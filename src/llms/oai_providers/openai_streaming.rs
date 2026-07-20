//! Shared SSE stream parsing and debug helpers for OpenAI-compatible providers.
//!
//! Extracted from `openai_compat.rs` to keep file sizes manageable.
//! Used by Grok, Groq, and `DeepSeek` streaming implementations.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ───────────────────────────────────────────────────────────────────
// Shared SSE stream parsing
// ───────────────────────────────────────────────────────────────────

/// Parsed SSE streaming response (OpenAI-compatible format).
#[derive(Debug, Deserialize)]
pub(crate) struct StreamResponse {
    /// List of completion choices returned by the API.
    pub choices: Vec<StreamChoice>,
    /// Optional token usage statistics.
    pub usage: Option<StreamUsage>,
}

/// A single choice from a streaming response.
#[derive(Debug, Deserialize)]
pub(crate) struct StreamChoice {
    /// Incremental content delta for this choice.
    pub delta: Option<StreamDelta>,
    /// Reason the model stopped generating (e.g. `"stop"`, `"tool_calls"`).
    pub finish_reason: Option<String>,
}

/// Incremental delta content within a streaming choice.
#[derive(Debug, Deserialize)]
pub(crate) struct StreamDelta {
    /// Text content fragment.
    pub content: Option<String>,
    /// Tool call fragments being streamed.
    pub tool_calls: Option<Vec<StreamToolCall>>,
}

/// A single tool call delta from a streaming response.
#[derive(Debug, Deserialize)]
pub(crate) struct StreamToolCall {
    /// Index of this tool call within the batch.
    pub index: Option<usize>,
    /// Unique identifier assigned by the API.
    pub id: Option<String>,
    /// Function name and argument fragments.
    pub function: Option<StreamFunctionDelta>,
}

/// Incremental function name and arguments within a tool call delta.
#[derive(Debug, Deserialize)]
pub(crate) struct StreamFunctionDelta {
    /// Function name (sent once at the start of the tool call).
    pub name: Option<String>,
    /// Partial JSON argument string.
    pub arguments: Option<String>,
}

/// Token usage statistics from a streaming response.
#[derive(Debug, Deserialize)]
pub(crate) struct StreamUsage {
    /// Number of prompt tokens consumed.
    #[serde(rename = "prompt_tokens")]
    pub prompt: Option<usize>,
    /// Number of completion tokens generated.
    #[serde(rename = "completion_tokens")]
    pub completion: Option<usize>,
    /// DeepSeek-specific cache fields
    #[serde(rename = "prompt_cache_hit_tokens")]
    pub prompt_cache_hit: Option<usize>,
    /// `DeepSeek`-specific: prompt tokens that missed the cache.
    #[serde(rename = "prompt_cache_miss_tokens")]
    pub prompt_cache_miss: Option<usize>,
}

/// Normalize provider-specific stop reasons to our internal format.
pub(crate) fn normalize_stop_reason(reason: &str) -> String {
    match reason {
        "length" => "max_tokens".to_owned(),
        "stop" => "end_turn".to_owned(),
        "tool_calls" => "tool_use".to_owned(),
        other => other.to_owned(),
    }
}

/// Process a single SSE line, returning parsed `StreamResponse` if valid.
pub(crate) fn parse_sse_line(line: &str) -> Option<StreamResponse> {
    if !line.starts_with("data: ") {
        return None;
    }
    let json_str = line.get(6..).unwrap_or("");
    if json_str == "[DONE]" {
        return None;
    }
    serde_json::from_str(json_str).ok()
}

/// Accumulator for building tool calls from streaming deltas.
#[derive(Default)]
pub(crate) struct ToolCallAccumulator {
    /// Map from tool-call index to `(id, name, arguments)` triple.
    pub calls: std::collections::HashMap<usize, (String, String, String)>,
}

impl ToolCallAccumulator {
    /// Create a new empty accumulator.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Feed a streaming tool call delta. Returns `(name, args_so_far)` for
    /// progress reporting when the tool name is known.
    pub(crate) fn feed(&mut self, call: &StreamToolCall) -> Option<(String, String)> {
        let idx = call.index.unwrap_or(0);
        let entry = self.calls.entry(idx).or_insert_with(|| (String::new(), String::new(), String::new()));

        if let Some(id) = call.id.as_ref() {
            entry.0.clone_from(id);
        }
        if let Some(func) = call.function.as_ref() {
            if let Some(name) = func.name.as_ref() {
                entry.1.clone_from(name);
            }
            if let Some(args) = func.arguments.as_ref() {
                entry.2.push_str(args);
            }
        }

        // Report progress when we know the tool name
        if entry.1.is_empty() { None } else { Some((entry.1.clone(), entry.2.clone())) }
    }

    /// Drain all completed tool calls into `ToolUse` events.
    pub(crate) fn drain(&mut self) -> Vec<crate::infra::tools::ToolUse> {
        self.calls
            .drain()
            .filter_map(|(_, (id, name, arguments))| {
                if id.is_empty() || name.is_empty() {
                    return None;
                }
                let input: Value =
                    serde_json::from_str(&arguments).unwrap_or_else(|_| Value::Object(serde_json::Map::new()));
                Some(crate::infra::tools::ToolUse::new(id, name, input))
            })
            .collect()
    }
}

// ───────────────────────────────────────────────────────────────────
// Shared debug dump helper
// ───────────────────────────────────────────────────────────────────

/// Dump an API request to disk for debugging.
pub(crate) fn dump_request<T>(worker_id: &str, provider: &str, request: &T)
where
    T: Serialize,
{
    let dir = ".context-pilot/last_requests";
    let _r1 = std::fs::create_dir_all(dir);
    let path = format!("{dir}/{worker_id}_{provider}_last_request.json");
    let _r2 = std::fs::write(path, serde_json::to_string_pretty(request).unwrap_or_default());
}

// ───────────────────────────────────────────────────────────────────
// Shared SSE consume loop (Grok / Groq / DeepSeek)
// ───────────────────────────────────────────────────────────────────

use std::io::{BufRead as _, BufReader};
use std::sync::mpsc::Sender;

use super::super::StreamEvent;
use super::super::error::LlmError;

/// Token/stop-reason totals accumulated while consuming an OpenAI-compatible
/// SSE stream. Cache fields stay zero for providers that don't report them.
#[derive(Default)]
pub(crate) struct SseAccum {
    /// Prompt (input) tokens reported by the final usage frame.
    pub input_tokens: usize,
    /// Completion (output) tokens reported by the final usage frame.
    pub output_tokens: usize,
    /// Prompt tokens served from cache (DeepSeek only; else 0).
    pub cache_hit_tokens: usize,
    /// Prompt tokens that missed the cache (DeepSeek only; else 0).
    pub cache_miss_tokens: usize,
    /// Normalized stop reason from the last choice's `finish_reason`.
    pub stop_reason: Option<String>,
}

/// Fold a usage frame's token counts into `acc`.
const fn apply_usage(acc: &mut SseAccum, usage: &StreamUsage) {
    if let Some(inp) = usage.prompt {
        acc.input_tokens = inp;
    }
    if let Some(out) = usage.completion {
        acc.output_tokens = out;
    }
    if let Some(hit) = usage.prompt_cache_hit {
        acc.cache_hit_tokens = hit;
    }
    if let Some(miss) = usage.prompt_cache_miss {
        acc.cache_miss_tokens = miss;
    }
}

/// Emit content chunk + tool-progress events from one streaming delta.
fn emit_delta_events(delta: StreamDelta, tx: &Sender<StreamEvent>, tool_acc: &mut ToolCallAccumulator) {
    if let Some(content) = delta.content
        && !content.is_empty()
    {
        let _r = tx.send(StreamEvent::Chunk(content));
    }
    let Some(calls) = delta.tool_calls else { return };
    for call in &calls {
        if let Some((name, input_so_far)) = tool_acc.feed(call) {
            let _r = tx.send(StreamEvent::ToolProgress { name, input_so_far });
        }
    }
}

/// Process one streaming choice: emit content chunks + tool progress from the
/// delta, and (on `finish_reason`) record the stop reason + drain tool calls.
fn process_choice(
    choice: StreamChoice,
    tx: &Sender<StreamEvent>,
    tool_acc: &mut ToolCallAccumulator,
    acc: &mut SseAccum,
) {
    if let Some(delta) = choice.delta {
        emit_delta_events(delta, tx, tool_acc);
    }
    if let Some(reason) = choice.finish_reason.as_ref() {
        acc.stop_reason = Some(normalize_stop_reason(reason));
        for tool_use in tool_acc.drain() {
            let _r = tx.send(StreamEvent::ToolUse(tool_use));
        }
    }
}

/// Fold one parsed SSE frame into `acc` + emit its chunks/tool events.
fn process_response(
    resp: StreamResponse,
    tx: &Sender<StreamEvent>,
    tool_acc: &mut ToolCallAccumulator,
    acc: &mut SseAccum,
) {
    if let Some(usage) = resp.usage {
        apply_usage(acc, &usage);
    }
    for choice in resp.choices {
        process_choice(choice, tx, tool_acc, acc);
    }
}

/// Read an OpenAI-compatible SSE response to completion, streaming chunks and
/// tool events to `tx`. Returns accumulated token/stop-reason totals. The caller
/// sends the terminal `StreamEvent::Done` from the returned `SseAccum`.
pub(crate) fn consume_sse_stream<R>(
    reader: BufReader<R>,
    tx: &Sender<StreamEvent>,
    tool_acc: &mut ToolCallAccumulator,
) -> Result<SseAccum, LlmError>
where
    R: std::io::Read,
{
    let mut acc = SseAccum::default();
    for raw_line in reader.lines() {
        let line = raw_line.map_err(|e| LlmError::StreamRead(e.to_string()))?;
        if let Some(resp) = parse_sse_line(&line) {
            process_response(resp, tx, tool_acc, &mut acc);
        }
    }
    Ok(acc)
}

// ───────────────────────────────────────────────────────────────────
// Shared request/probe helpers (Grok / Groq / DeepSeek)
// ───────────────────────────────────────────────────────────────────

use reqwest::blocking::Client;
use serde_json::json;

/// One OpenAI-compatible endpoint: HTTP client + URL + bearer key. Bundling
/// these three keeps the shared helpers under the 4-argument cap.
pub(crate) struct OaiEndpoint<'ep> {
    /// Blocking HTTP client to issue requests with.
    pub client: &'ep Client,
    /// Chat-completions endpoint URL.
    pub url: &'ep str,
    /// Bearer API key (already exposed).
    pub key: &'ep str,
}

/// POST `request` to the endpoint and consume the SSE response to completion,
/// streaming chunks + tool events to `tx`. Returns the accumulated token/stop
/// totals, or an [`LlmError`] on a transport failure or non-2xx status.
pub(crate) fn run_oai_stream<T>(
    ep: &OaiEndpoint<'_>,
    request: &T,
    tx: &Sender<StreamEvent>,
) -> Result<SseAccum, LlmError>
where
    T: Serialize,
{
    let response = ep
        .client
        .post(ep.url)
        .header("Authorization", format!("Bearer {}", ep.key))
        .header("Content-Type", "application/json")
        .json(request)
        .send()?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().unwrap_or_default();
        return Err(LlmError::Api { status, body });
    }

    let reader = BufReader::new(response);
    let mut tool_acc = ToolCallAccumulator::new();
    consume_sse_stream(reader, tx, &mut tool_acc)
}

/// Send the terminal [`StreamEvent::Done`] from an accumulated `SseAccum`.
/// Cache-hit/miss come straight from `acc` (zero for providers that don't
/// report them); breakpoint + alive fields are empty for OpenAI-compatible APIs.
pub(crate) fn send_stream_done(tx: &Sender<StreamEvent>, acc: SseAccum) {
    let _r = tx.send(StreamEvent::Done {
        input_tokens: acc.input_tokens,
        output_tokens: acc.output_tokens,
        cache_hit_tokens: acc.cache_hit_tokens,
        cache_miss_tokens: acc.cache_miss_tokens,
        stop_reason: acc.stop_reason,
        bp_hashes: vec![],
        bp_panel_ids: vec![],
        alive_count: 0,
        alive_positions_permille: vec![],
    });
}

/// Issue a single probe request, returning `true` on a 2xx response.
fn probe_ok(ep: &OaiEndpoint<'_>, body: &Value) -> bool {
    ep.client
        .post(ep.url)
        .header("Authorization", format!("Bearer {}", ep.key))
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .is_ok_and(|r| r.status().is_success())
}

/// Shared `check_api` for OpenAI-compatible providers: runs the auth, streaming,
/// and tools probes against `ep`. `tokens_field` selects the max-tokens key name
/// (`"max_tokens"` vs Groq's `"max_completion_tokens"`).
pub(crate) fn oai_check_api(ep: &OaiEndpoint<'_>, model: &str, tokens_field: &str) -> super::super::ApiCheckResult {
    let auth_body = json!({ "model": model, tokens_field: 10i32, "messages": [{"role": "user", "content": "Hi"}] });
    if !probe_ok(ep, &auth_body) {
        return super::super::ApiCheckResult::failure(Some("Auth failed".to_owned()));
    }

    let stream_body = json!({
        "model": model, tokens_field: 10i32, "stream": true,
        "messages": [{"role": "user", "content": "Say ok"}]
    });
    let streaming_ok = probe_ok(ep, &stream_body);

    let tools_body = json!({
        "model": model, tokens_field: 50i32,
        "tools": [{
            "type": "function",
            "function": {
                "name": "test_tool", "description": "A test tool",
                "parameters": { "type": "object", "properties": {}, "required": [] }
            }
        }],
        "messages": [{"role": "user", "content": "Hi"}]
    });
    let tools_ok = probe_ok(ep, &tools_body);

    let mut r = super::super::ApiCheckResult::default();
    r.auth_ok = true;
    r.streaming_ok = streaming_ok;
    r.tools_ok = tools_ok;
    r
}
