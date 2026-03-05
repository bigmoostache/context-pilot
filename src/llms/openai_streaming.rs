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
        "length" => "max_tokens".to_string(),
        "stop" => "end_turn".to_string(),
        "tool_calls" => "tool_use".to_string(),
        other => other.to_string(),
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

        if let Some(ref id) = call.id {
            entry.0.clone_from(id);
        }
        if let Some(ref func) = call.function {
            if let Some(ref name) = func.name {
                entry.1.clone_from(name);
            }
            if let Some(ref args) = func.arguments {
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
                Some(crate::infra::tools::ToolUse { id, name, input })
            })
            .collect()
    }
}

// ───────────────────────────────────────────────────────────────────
// Shared debug dump helper
// ───────────────────────────────────────────────────────────────────

/// Dump an API request to disk for debugging.
pub(crate) fn dump_request<T: Serialize>(worker_id: &str, provider: &str, request: &T) {
    let dir = ".context-pilot/last_requests";
    let _r1 = std::fs::create_dir_all(dir);
    let path = format!("{dir}/{worker_id}_{provider}_last_request.json");
    let _r2 = std::fs::write(path, serde_json::to_string_pretty(request).unwrap_or_default());
}
