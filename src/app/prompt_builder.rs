//! Centralized prompt assembly — the ONE place that builds the full LLM prompt.
//!
//! Takes raw context data (panels, messages, tools, system prompt) and assembles
//! the final `Vec<ApiMessage>` that any LLM provider can serialize to its wire format.
//!
//! Previously this logic was duplicated across 3+ provider implementations:
//! - `anthropic/messages.rs` — `messages_to_api()`
//! - `claude_code/stream.rs` — inline panel injection
//! - `openai_compat.rs` — `build_messages()`
//!
//! Now: ONE function, ONE representation, all providers just serialize.

use crate::app::panels::ContextItem;
use crate::llms::{
    ApiMessage, ContentBlock, panel_footer_text, panel_header_text, panel_timestamp_text, prepare_panel_messages,
};
use crate::state::{Message, MsgKind, MsgStatus};
use cp_base::config::INJECTIONS;

/// Assemble the full prompt as `Vec<ApiMessage>`.
///
/// This is the canonical prompt builder — called from `prepare_stream_context()`.
/// The returned messages contain:
/// 1. Panel injection (fake `tool_use/result` pairs, sorted by timestamp)
/// 2. Panel footer with message timestamps
/// 3. Seed content re-injection (system prompt repeated after panels)
/// 4. Conversation messages (with tool pairing — orphaned `tool_uses` excluded)
///
/// Providers receive this and just serialize to their wire format.
pub(crate) fn assemble_prompt(
    messages: &[Message],
    context_items: &[ContextItem],
    include_last_tool_uses: bool,
    seed_content: Option<&str>,
) -> Vec<ApiMessage> {
    let mut api_messages: Vec<ApiMessage> = Vec::new();
    let current_ms = crate::app::panels::now_ms();

    // ── Phase 1: Panel injection ────────────────────────────────
    // Each panel becomes an assistant tool_use + user tool_result pair.
    // Sorted by timestamp (oldest first, newest closest to conversation).
    let fake_panels = prepare_panel_messages(context_items);

    if !fake_panels.is_empty() {
        inject_panel_messages(
            &mut api_messages,
            &PanelInjection { fake_panels: &fake_panels, current_ms, seed_content },
        );
    }

    // ── Phase 2: Conversation messages ──────────────────────────
    for (idx, msg) in messages.iter().enumerate() {
        if msg.status == MsgStatus::Deleted || msg.status == MsgStatus::Detached {
            continue;
        }

        if msg.content.is_empty() && msg.tool_uses.is_empty() && msg.tool_results.is_empty() {
            continue;
        }

        let mut content_blocks: Vec<ContentBlock> = Vec::new();

        if msg.msg_type == MsgKind::ToolResult {
            for result in &msg.tool_results {
                content_blocks.push(ContentBlock::ToolResult {
                    tool_use_id: result.tool_use_id.clone(),
                    content: result.content.clone(),
                });
            }
            if !content_blocks.is_empty() {
                api_messages.push(ApiMessage { role: "user".to_string(), content: content_blocks });
            }
            continue;
        }

        if msg.msg_type == MsgKind::ToolCall {
            if let Some(blocks) = build_tool_call_blocks(msg, messages, idx) {
                if let Some(last_api_msg) = api_messages.last_mut()
                    && last_api_msg.role == "assistant"
                {
                    last_api_msg.content.extend(blocks);
                    continue;
                }
                content_blocks = blocks;
            } else {
                continue;
            }
        } else {
            let message_content = msg.content.clone();

            if !message_content.is_empty() {
                content_blocks.push(ContentBlock::Text { text: message_content });
            }

            let is_last = idx == messages.len().saturating_sub(1);
            if msg.role == "assistant" && include_last_tool_uses && is_last && !msg.tool_uses.is_empty() {
                for tool_use in &msg.tool_uses {
                    content_blocks.push(tool_use_block(tool_use));
                }
            }
        }

        if !content_blocks.is_empty() {
            api_messages.push(ApiMessage { role: msg.role.clone(), content: content_blocks });
        }
    }

    api_messages
}

// ── Panel injection ─────────────────────────────────────────────

/// Context needed for panel injection into the prompt.
struct PanelInjection<'inj> {
    /// Fake panel messages to inject as tool call/result pairs.
    fake_panels: &'inj [crate::llms::FakePanelMessage],
    /// Current timestamp in milliseconds.
    current_ms: u64,
    /// Optional seed content to re-inject after panels.
    seed_content: Option<&'inj str>,
}

/// Inject context panels as fake tool call/result message pairs.
fn inject_panel_messages(api_messages: &mut Vec<ApiMessage>, ctx: &PanelInjection<'_>) {
    for (idx, panel) in ctx.fake_panels.iter().enumerate() {
        let timestamp_text = panel_timestamp_text(panel.timestamp_ms);
        let text = if idx == 0 { format!("{}\n\n{}", panel_header_text(), timestamp_text) } else { timestamp_text };

        api_messages.push(ApiMessage {
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Text { text },
                ContentBlock::ToolUse {
                    id: format!("panel_{}", panel.panel_id),
                    name: "dynamic_panel".to_string(),
                    input: serde_json::json!({ "id": panel.panel_id }),
                },
            ],
        });
        api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: vec![ContentBlock::ToolResult {
                tool_use_id: format!("panel_{}", panel.panel_id),
                content: panel.content.clone(),
            }],
        });
    }

    // Footer after all panels
    let footer = panel_footer_text(ctx.current_ms);
    api_messages.push(ApiMessage {
        role: "assistant".to_string(),
        content: vec![
            ContentBlock::Text { text: footer },
            ContentBlock::ToolUse {
                id: "panel_footer".to_string(),
                name: "dynamic_panel".to_string(),
                input: serde_json::json!({ "action": "end_panels" }),
            },
        ],
    });
    api_messages.push(ApiMessage {
        role: "user".to_string(),
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "panel_footer".to_string(),
            content: crate::infra::constants::prompts::panel_footer_ack().to_string(),
        }],
    });

    // Re-inject seed/system prompt after panels
    if let Some(seed) = ctx.seed_content {
        let header = &INJECTIONS.providers.seed_reinjection_header;
        api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: vec![ContentBlock::Text { text: format!("{header}\n\n{seed}") }],
        });
        api_messages.push(ApiMessage {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text { text: INJECTIONS.providers.seed_reinjection_ack.clone() }],
        });
    }
}

// ── Tool pairing helpers ────────────────────────────────────────

/// Build `ContentBlocks` for a `ToolCall` message, if it has a matching `ToolResult`.
fn build_tool_call_blocks(msg: &Message, messages: &[Message], idx: usize) -> Option<Vec<ContentBlock>> {
    let tool_use_ids: Vec<&str> = msg.tool_uses.iter().map(|t| t.id.as_str()).collect();

    let rest = messages.get(idx.saturating_add(1)..).unwrap_or_default();
    let has_matching_result = rest
        .iter()
        .filter(|m| m.status != MsgStatus::Deleted && m.status != MsgStatus::Detached)
        .filter(|m| m.msg_type == MsgKind::ToolResult)
        .any(|m| m.tool_results.iter().any(|r| tool_use_ids.contains(&r.tool_use_id.as_str())));

    if !has_matching_result {
        return None;
    }

    Some(msg.tool_uses.iter().map(tool_use_block).collect())
}

/// Convert a `ToolUseRecord` into a `ContentBlock`, ensuring input is never null.
fn tool_use_block(tool_use: &crate::state::ToolUseRecord) -> ContentBlock {
    let input = if tool_use.input.is_null() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        tool_use.input.clone()
    };
    ContentBlock::ToolUse { id: tool_use.id.clone(), name: tool_use.name.clone(), input }
}
