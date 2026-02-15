//! Shared OpenAI-compatible message builder.
//!
//! Grok, Groq, and DeepSeek all use the OpenAI chat completions format.
//! This module extracts the common message-building logic so each provider
//! only needs to handle its own quirks (request struct, endpoint, headers).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{panel_footer_text, panel_header_text, panel_timestamp_text, prepare_panel_messages};
use crate::constants::{library, prompts};
use crate::core::panels::now_ms;
use crate::state::{Message, MessageStatus, MessageType};
use crate::tool_defs::ToolDefinition;

// ───────────────────────────────────────────────────────────────────
// Shared message type
// ───────────────────────────────────────────────────────────────────

/// OpenAI-compatible chat message.
#[derive(Debug, Clone, Serialize)]
pub struct OaiMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OaiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OaiToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: OaiFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OaiFunction {
    pub name: String,
    pub arguments: String,
}

/// OpenAI-compatible tool definition wrapper.
#[derive(Debug, Serialize)]
pub struct OaiTool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: OaiFunctionDef,
}

#[derive(Debug, Serialize)]
pub struct OaiFunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

// ───────────────────────────────────────────────────────────────────
// Shared tool-pairing helper (used by ALL providers)
// ───────────────────────────────────────────────────────────────────

/// Collect the set of tool_use IDs that have matching tool_result messages.
///
/// Tool calls without results (e.g. truncated by max_tokens) must be excluded
/// to avoid provider-specific "insufficient tool messages" API errors.
///
/// `pending_tool_result_ids` are IDs from the current tool loop that haven't
/// been persisted as messages yet but will be sent as separate tool results.
pub fn collect_included_tool_ids(messages: &[Message], pending_tool_result_ids: &[String]) -> HashSet<String> {
    let mut included: HashSet<String> = pending_tool_result_ids.iter().cloned().collect();

    for (idx, msg) in messages.iter().enumerate() {
        if msg.status == MessageStatus::Deleted
            || msg.status == MessageStatus::Detached
            || msg.message_type != MessageType::ToolCall
        {
            continue;
        }

        let tool_use_ids: Vec<&str> = msg.tool_uses.iter().map(|t| t.id.as_str()).collect();

        let has_result = messages[idx + 1..]
            .iter()
            .filter(|m| {
                m.status != MessageStatus::Deleted
                    && m.status != MessageStatus::Detached
                    && m.message_type == MessageType::ToolResult
            })
            .any(|m| m.tool_results.iter().any(|r| tool_use_ids.contains(&r.tool_use_id.as_str())));

        if has_result {
            for id in tool_use_ids {
                included.insert(id.to_string());
            }
        }
    }

    included
}

// ───────────────────────────────────────────────────────────────────
// OpenAI-compat message builder
// ───────────────────────────────────────────────────────────────────

/// Options for customizing the shared message builder per-provider.
pub struct BuildOptions {
    /// System prompt text (falls back to default if None).
    pub system_prompt: Option<String>,
    /// Extra text appended to system message (e.g. Groq's built-in tools info).
    pub system_suffix: Option<String>,
    /// Extra context for cleaner mode.
    pub extra_context: Option<String>,
    /// Pending tool result IDs from current tool loop.
    pub pending_tool_result_ids: Vec<String>,
}

/// Build the full OpenAI-compatible message list.
///
/// Handles: system message, panel injection, tool pairing, message
/// conversion with [ID]: prefixes, extra context, footer/header.
pub fn build_messages(
    messages: &[Message],
    context_items: &[crate::core::panels::ContextItem],
    opts: &BuildOptions,
) -> Vec<OaiMessage> {
    let mut out: Vec<OaiMessage> = Vec::new();

    // ── System message ──────────────────────────────────────────
    let mut system_content = opts.system_prompt.clone().unwrap_or_else(|| library::default_agent_content().to_string());

    if let Some(ref suffix) = opts.system_suffix {
        system_content.push_str("\n\n");
        system_content.push_str(suffix);
    }

    out.push(OaiMessage {
        role: "system".to_string(),
        content: Some(system_content),
        tool_calls: None,
        tool_call_id: None,
    });

    // ── Panel injection ─────────────────────────────────────────
    let fake_panels = prepare_panel_messages(context_items);
    let current_ms = now_ms();

    if !fake_panels.is_empty() {
        for (idx, panel) in fake_panels.iter().enumerate() {
            let timestamp_text = panel_timestamp_text(panel.timestamp_ms);
            let text = if idx == 0 { format!("{}\n\n{}", panel_header_text(), timestamp_text) } else { timestamp_text };

            // Assistant message with tool_call
            out.push(OaiMessage {
                role: "assistant".to_string(),
                content: Some(text),
                tool_calls: Some(vec![OaiToolCall {
                    id: format!("panel_{}", panel.panel_id),
                    call_type: "function".to_string(),
                    function: OaiFunction {
                        name: "dynamic_panel".to_string(),
                        arguments: format!(r#"{{"id":"{}"}}"#, panel.panel_id),
                    },
                }]),
                tool_call_id: None,
            });

            // Tool result message
            out.push(OaiMessage {
                role: "tool".to_string(),
                content: Some(panel.content.clone()),
                tool_calls: None,
                tool_call_id: Some(format!("panel_{}", panel.panel_id)),
            });
        }

        // Footer after all panels
        let footer = panel_footer_text(messages, current_ms);
        out.push(OaiMessage {
            role: "assistant".to_string(),
            content: Some(footer),
            tool_calls: Some(vec![OaiToolCall {
                id: "panel_footer".to_string(),
                call_type: "function".to_string(),
                function: OaiFunction {
                    name: "dynamic_panel".to_string(),
                    arguments: r#"{"action":"end_panels"}"#.to_string(),
                },
            }]),
            tool_call_id: None,
        });
        out.push(OaiMessage {
            role: "tool".to_string(),
            content: Some(prompts::panel_footer_ack().to_string()),
            tool_calls: None,
            tool_call_id: Some("panel_footer".to_string()),
        });
    }

    // ── Extra context (cleaner mode) ────────────────────────────
    if let Some(ref ctx) = opts.extra_context {
        out.push(OaiMessage {
            role: "user".to_string(),
            content: Some(format!("Please clean up the context to reduce token usage:\n\n{}", ctx)),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    // ── Tool pairing ────────────────────────────────────────────
    let included_tool_ids = collect_included_tool_ids(messages, &opts.pending_tool_result_ids);

    // ── Conversation messages ───────────────────────────────────
    for msg in messages.iter() {
        if msg.status == MessageStatus::Deleted || msg.status == MessageStatus::Detached {
            continue;
        }
        if msg.content.is_empty() && msg.tool_uses.is_empty() && msg.tool_results.is_empty() {
            continue;
        }

        // Tool results
        if msg.message_type == MessageType::ToolResult {
            for result in &msg.tool_results {
                if included_tool_ids.contains(&result.tool_use_id) {
                    out.push(OaiMessage {
                        role: "tool".to_string(),
                        content: Some(result.content.clone()),
                        tool_calls: None,
                        tool_call_id: Some(result.tool_use_id.clone()),
                    });
                }
            }
            continue;
        }

        // Tool calls — only include if they have matching results.
        // Merge into the last assistant message if possible, so consecutive
        // tool calls from the same turn become one assistant message with
        // multiple tool_calls (required by OpenAI-compat APIs).
        if msg.message_type == MessageType::ToolCall {
            let calls: Vec<OaiToolCall> = msg
                .tool_uses
                .iter()
                .filter(|tu| included_tool_ids.contains(&tu.id))
                .map(|tu| OaiToolCall {
                    id: tu.id.clone(),
                    call_type: "function".to_string(),
                    function: OaiFunction {
                        name: tu.name.clone(),
                        arguments: serde_json::to_string(&tu.input).unwrap_or_default(),
                    },
                })
                .collect();

            if !calls.is_empty() {
                // Try to merge into the last assistant message so consecutive
                // tool calls become one assistant message (required by OpenAI APIs)
                let should_merge = out.last().is_some_and(|last| last.role == "assistant" && last.tool_calls.is_some());

                if should_merge {
                    if let Some(last) = out.last_mut()
                        && let Some(ref mut existing_calls) = last.tool_calls
                    {
                        existing_calls.extend(calls);
                    }
                } else {
                    out.push(OaiMessage {
                        role: "assistant".to_string(),
                        content: None,
                        tool_calls: Some(calls),
                        tool_call_id: None,
                    });
                }
            }
            continue;
        }

        // Regular text message
        let message_content = match msg.status {
            MessageStatus::Summarized => msg.tl_dr.as_ref().unwrap_or(&msg.content).clone(),
            _ => msg.content.clone(),
        };

        if !message_content.is_empty() {
            out.push(OaiMessage {
                role: msg.role.clone(),
                content: Some(message_content),
                tool_calls: None,
                tool_call_id: None,
            });
        }
    }

    out
}

// ───────────────────────────────────────────────────────────────────
// Shared tool definition converter
// ───────────────────────────────────────────────────────────────────

/// Convert internal tool definitions to OpenAI-compatible format.
pub fn tools_to_oai(tools: &[ToolDefinition]) -> Vec<OaiTool> {
    tools
        .iter()
        .filter(|t| t.enabled)
        .map(|t| OaiTool {
            tool_type: "function".to_string(),
            function: OaiFunctionDef {
                name: t.id.clone(),
                description: t.description.clone(),
                parameters: t.to_json_schema(),
            },
        })
        .collect()
}

// ───────────────────────────────────────────────────────────────────
// Shared SSE stream parsing
// ───────────────────────────────────────────────────────────────────

/// Parsed SSE streaming response (OpenAI-compatible format).
#[derive(Debug, Deserialize)]
pub struct StreamResponse {
    pub choices: Vec<StreamChoice>,
    pub usage: Option<StreamUsage>,
}

#[derive(Debug, Deserialize)]
pub struct StreamChoice {
    pub delta: Option<StreamDelta>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StreamDelta {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Debug, Deserialize)]
pub struct StreamToolCall {
    pub index: Option<usize>,
    pub id: Option<String>,
    pub function: Option<StreamFunctionDelta>,
}

#[derive(Debug, Deserialize)]
pub struct StreamFunctionDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StreamUsage {
    pub prompt_tokens: Option<usize>,
    pub completion_tokens: Option<usize>,
    /// DeepSeek-specific cache fields
    pub prompt_cache_hit_tokens: Option<usize>,
    pub prompt_cache_miss_tokens: Option<usize>,
}

/// Normalize provider-specific stop reasons to our internal format.
pub fn normalize_stop_reason(reason: &str) -> String {
    match reason {
        "length" => "max_tokens".to_string(),
        "stop" => "end_turn".to_string(),
        "tool_calls" => "tool_use".to_string(),
        other => other.to_string(),
    }
}

/// Process a single SSE line, returning parsed StreamResponse if valid.
pub fn parse_sse_line(line: &str) -> Option<StreamResponse> {
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
pub struct ToolCallAccumulator {
    pub calls: std::collections::HashMap<usize, (String, String, String)>,
}

impl ToolCallAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a streaming tool call delta.
    pub fn feed(&mut self, call: &StreamToolCall) {
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
    pub fn drain(&mut self) -> Vec<crate::tools::ToolUse> {
        self.calls
            .drain()
            .filter_map(|(_, (id, name, arguments))| {
                if id.is_empty() || name.is_empty() {
                    return None;
                }
                let input: Value =
                    serde_json::from_str(&arguments).unwrap_or_else(|_| Value::Object(serde_json::Map::new()));
                Some(crate::tools::ToolUse { id, name, input })
            })
            .collect()
    }
}

// ───────────────────────────────────────────────────────────────────
// Shared debug dump helper
// ───────────────────────────────────────────────────────────────────

/// Dump an API request to disk for debugging.
pub fn dump_request<T: Serialize>(worker_id: &str, provider: &str, request: &T) {
    let dir = ".context-pilot/last_requests";
    let _ = std::fs::create_dir_all(dir);
    let path = format!("{}/{}_{}_last_request.json", dir, worker_id, provider);
    let _ = std::fs::write(path, serde_json::to_string_pretty(request).unwrap_or_default());
}
