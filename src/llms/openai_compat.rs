//! Shared OpenAI-compatible message builder.
//!
//! Grok, Groq, and `DeepSeek` all use the `OpenAI` chat completions format.
//! This module extracts the common message-building logic so each provider
//! only needs to handle its own quirks (request struct, endpoint, headers).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{panel_footer_text, panel_header_text, panel_timestamp_text, prepare_panel_messages};
use crate::app::panels::now_ms;
use crate::infra::constants::{library, prompts};
use crate::infra::tools::ToolDefinition;
use crate::state::{Message, MsgKind, MsgStatus};
use cp_base::config::INJECTIONS;

// ───────────────────────────────────────────────────────────────────
// Shared message type
// ───────────────────────────────────────────────────────────────────

/// OpenAI-compatible chat message.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct OaiMessage {
    /// Message role (`"system"`, `"user"`, `"assistant"`, or `"tool"`).
    pub role: String,
    /// Text content of the message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Tool calls made by the assistant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OaiToolCall>>,
    /// ID of the tool call this message is a result for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// A single tool call issued by the assistant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct OaiToolCall {
    /// Unique identifier for this tool call.
    pub id: String,
    /// Always `"function"` for function-based tool calls.
    #[serde(rename = "type")]
    pub call_type: String,
    /// Function name and serialized arguments.
    pub function: OaiFunction,
}

/// Function name and JSON-encoded arguments within a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct OaiFunction {
    /// Name of the function to invoke.
    pub name: String,
    /// JSON-serialized argument string.
    pub arguments: String,
}

/// OpenAI-compatible tool definition wrapper.
#[derive(Debug, Serialize)]
pub(crate) struct OaiTool {
    /// Always `"function"` for function-based tools.
    #[serde(rename = "type")]
    pub tool_type: String,
    /// Function metadata (name, description, JSON-schema parameters).
    pub function: OaiFunctionDef,
}

/// Function metadata within an OpenAI-compatible tool definition.
#[derive(Debug, Serialize)]
pub(crate) struct OaiFunctionDef {
    /// Function identifier.
    pub name: String,
    /// Human-readable description sent to the model.
    pub description: String,
    /// JSON Schema describing accepted parameters.
    pub parameters: Value,
}

// ───────────────────────────────────────────────────────────────────
// Shared tool-pairing helper (used by ALL providers)
// ───────────────────────────────────────────────────────────────────

/// Collect the set of `tool_use` IDs that have matching `tool_result` messages.
///
/// Tool calls without results (e.g. truncated by `max_tokens`) must be excluded
/// to avoid provider-specific "insufficient tool messages" API errors.
///
/// `pending_tool_result_ids` are IDs from the current tool loop that haven't
/// been persisted as messages yet but will be sent as separate tool results.
pub(crate) fn collect_included_tool_ids(messages: &[Message], pending_tool_result_ids: &[String]) -> HashSet<String> {
    let mut included: HashSet<String> = pending_tool_result_ids.iter().cloned().collect();

    for (idx, msg) in messages.iter().enumerate() {
        if msg.status == MsgStatus::Deleted || msg.status == MsgStatus::Detached || msg.msg_type != MsgKind::ToolCall {
            continue;
        }

        let tool_use_ids: Vec<&str> = msg.tool_uses.iter().map(|t| t.id.as_str()).collect();

        let has_result = messages
            .get(idx.saturating_add(1)..)
            .unwrap_or_default()
            .iter()
            .filter(|m: &&Message| {
                m.status != MsgStatus::Deleted && m.status != MsgStatus::Detached && m.msg_type == MsgKind::ToolResult
            })
            .any(|m: &Message| m.tool_results.iter().any(|r| tool_use_ids.contains(&r.tool_use_id.as_str())));

        if has_result {
            for id in tool_use_ids {
                let _r = included.insert(id.to_string());
            }
        }
    }

    included
}

// ───────────────────────────────────────────────────────────────────
// OpenAI-compat message builder
// ───────────────────────────────────────────────────────────────────

/// Options for customizing the shared message builder per-provider.
pub(crate) struct BuildOptions {
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
/// When pre-assembled API messages are available, converts them to OAI format.
/// Falls back to building from raw data (for `api_check` etc.).
pub(crate) fn build_messages(
    messages: &[Message],
    context_items: &[crate::app::panels::ContextItem],
    opts: &BuildOptions,
    api_messages: &[super::ApiMessage],
) -> Vec<OaiMessage> {
    // If we have pre-assembled API messages, convert them directly
    if !api_messages.is_empty() {
        return build_from_api_messages(api_messages, opts);
    }

    // Fallback: build from raw data (legacy path, for api_check etc.)
    build_from_raw(messages, context_items, opts)
}

/// Convert pre-assembled `Vec<ApiMessage>` to OpenAI-compatible format.
fn build_from_api_messages(api_messages: &[super::ApiMessage], opts: &BuildOptions) -> Vec<OaiMessage> {
    let mut out: Vec<OaiMessage> = Vec::new();

    // System message
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

    // Convert each ApiMessage
    for msg in api_messages {
        // Check if it contains tool_use blocks → convert to tool_calls
        let has_tool_use = msg.content.iter().any(|b| matches!(b, super::ContentBlock::ToolUse { .. }));
        let has_tool_result = msg.content.iter().any(|b| matches!(b, super::ContentBlock::ToolResult { .. }));

        if has_tool_result {
            // Tool results → individual tool messages
            for block in &msg.content {
                if let super::ContentBlock::ToolResult { tool_use_id, content } = block {
                    out.push(OaiMessage {
                        role: "tool".to_string(),
                        content: Some(content.clone()),
                        tool_calls: None,
                        tool_call_id: Some(tool_use_id.clone()),
                    });
                }
            }
        } else if has_tool_use {
            // Assistant message with text + tool calls
            let text_parts: Vec<&str> = msg
                .content
                .iter()
                .filter_map(|b| if let super::ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
                .collect();
            let text = if text_parts.is_empty() { None } else { Some(text_parts.join("\n")) };

            let calls: Vec<OaiToolCall> = msg
                .content
                .iter()
                .filter_map(|b| {
                    if let super::ContentBlock::ToolUse { id, name, input } = b {
                        Some(OaiToolCall {
                            id: id.clone(),
                            call_type: "function".to_string(),
                            function: OaiFunction {
                                name: name.clone(),
                                arguments: serde_json::to_string(input).unwrap_or_default(),
                            },
                        })
                    } else {
                        None
                    }
                })
                .collect();

            out.push(OaiMessage {
                role: msg.role.clone(),
                content: text,
                tool_calls: if calls.is_empty() { None } else { Some(calls) },
                tool_call_id: None,
            });
        } else {
            // Pure text message
            let text: String = msg
                .content
                .iter()
                .filter_map(|b| if let super::ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
                .collect::<Vec<_>>()
                .join("\n");

            if !text.is_empty() {
                out.push(OaiMessage {
                    role: msg.role.clone(),
                    content: Some(text),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
        }
    }

    // Extra context (cleaner mode)
    if let Some(ref ctx) = opts.extra_context {
        let msg = INJECTIONS.providers.cleaner_mode.trim_end().replace("{context}", ctx);
        out.push(OaiMessage { role: "user".to_string(), content: Some(msg), tool_calls: None, tool_call_id: None });
    }

    out
}

/// Build from raw messages + `context_items` (legacy fallback path).
fn build_from_raw(
    messages: &[Message],
    context_items: &[crate::app::panels::ContextItem],
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
        let footer = panel_footer_text(current_ms);
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
        let msg = INJECTIONS.providers.cleaner_mode.trim_end().replace("{context}", ctx);
        out.push(OaiMessage { role: "user".to_string(), content: Some(msg), tool_calls: None, tool_call_id: None });
    }

    // ── Tool pairing ────────────────────────────────────────────
    let included_tool_ids = collect_included_tool_ids(messages, &opts.pending_tool_result_ids);

    // ── Conversation messages ───────────────────────────────────
    for msg in messages {
        if msg.status == MsgStatus::Deleted || msg.status == MsgStatus::Detached {
            continue;
        }
        if msg.content.is_empty() && msg.tool_uses.is_empty() && msg.tool_results.is_empty() {
            continue;
        }

        // Tool results
        if msg.msg_type == MsgKind::ToolResult {
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
        if msg.msg_type == MsgKind::ToolCall {
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

        let message_content = msg.content.clone();

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
pub(crate) fn tools_to_oai(tools: &[ToolDefinition]) -> Vec<OaiTool> {
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
