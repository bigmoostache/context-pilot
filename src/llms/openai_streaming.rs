//! Shared SSE stream parsing and debug helpers for OpenAI-compatible providers.
//!
//! Extracted from `openai_compat.rs` to keep file sizes manageable.
//! Used by Grok, Groq, and DeepSeek streaming implementations.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ───────────────────────────────────────────────────────────────────
// Shared SSE stream parsing
// ───────────────────────────────────────────────────────────────────

/// Parsed SSE streaming response (OpenAI-compatible format).
#[derive(Debug, Deserialize)]
pub(crate) struct StreamResponse {
    pub choices: Vec<StreamChoice>,
    pub usage: Option<StreamUsage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StreamChoice {
    pub delta: Option<StreamDelta>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StreamDelta {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StreamToolCall {
    pub index: Option<usize>,
    pub id: Option<String>,
    pub function: Option<StreamFunctionDelta>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StreamFunctionDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StreamUsage {
    pub prompt_tokens: Option<usize>,
    pub completion_tokens: Option<usize>,
    /// DeepSeek-specific cache fields
    pub prompt_cache_hit_tokens: Option<usize>,
    pub prompt_cache_miss_tokens: Option<usize>,
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

/// Process a single SSE line, returning parsed StreamResponse if valid.
pub(crate) fn parse_sse_line(line: &str) -> Option<StreamResponse> {
    if !line.starts_with("data: ") {
        return None;
    }
    let json_str = &line[6..];
    if json_str == "[DONE]" {
        return None;
    }
    serde_json::from_str(json_str).ok()
}

/// Accumulator for building tool calls from streaming deltas.
#[derive(Default)]
pub(crate) struct ToolCallAccumulator {
    pub calls: std::collections::HashMap<usize, (String, String, String)>,
}

impl ToolCallAccumulator {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Feed a streaming tool call delta.
    pub(crate) fn feed(&mut self, call: &StreamToolCall) {
        let idx = call.index.unwrap_or(0);
        let entry = self.calls.entry(idx).or_insert_with(|| (String::new(), String::new(), String::new()));

        if let Some(ref id) = call.id {
            entry.0 = id.clone();
        }
        if let Some(ref func) = call.function {
            if let Some(ref name) = func.name {
                entry.1 = name.clone();
            }
            if let Some(ref args) = func.arguments {
                entry.2.push_str(args);
            }
        }
    }

    /// Drain all completed tool calls into ToolUse events.
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
    let _ = std::fs::create_dir_all(dir);
    let path = format!("{}/{}_{}_last_request.json", dir, worker_id, provider);
    let _ = std::fs::write(path, serde_json::to_string_pretty(request).unwrap_or_default());
}
