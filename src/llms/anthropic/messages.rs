//! Anthropic message conversion: internal messages → API format.

use serde_json::Value;

use crate::app::panels::now_ms;
use crate::llms::{
    ApiMessage, ContentBlock, panel_footer_text, panel_header_text, panel_timestamp_text, prepare_panel_messages,
};
use crate::state::{Message, MsgKind, MsgStatus};

/// Convert internal messages to Anthropic API format.
/// Context items are injected as fake tool call/result pairs at the start.
pub(in crate::llms) fn messages_to_api(
    messages: &[Message],
    context_items: &[crate::app::panels::ContextItem],
    include_last_tool_uses: bool,
    seed_content: Option<&str>,
) -> Vec<ApiMessage> {
    let mut api_messages: Vec<ApiMessage> = Vec::new();
    let current_ms = now_ms();

    // Inject context panels as fake tool call/result pairs (P2+ only, sorted by timestamp)
    let fake_panels = prepare_panel_messages(context_items);

    if !fake_panels.is_empty() {
        inject_panel_messages(
            &mut api_messages,
            &PanelInjection { fake_panels: &fake_panels, current_ms, seed_content },
        );
    }

    for (idx, msg) in messages.iter().enumerate() {
        push_converted_message(&mut api_messages, msg, &MsgConvertCtx { all: messages, idx, include_last_tool_uses });
    }

    api_messages
}

/// Positional context for converting one message: the full slice, this
/// message's index, and whether the last assistant turn's tool uses ship.
struct MsgConvertCtx<'ctx> {
    /// Full message slice (for tool-call/result pairing lookups).
    all: &'ctx [Message],
    /// Index of the message being converted.
    idx: usize,
    /// Whether the last assistant message's `tool_uses` are emitted.
    include_last_tool_uses: bool,
}

/// Convert one internal message and push the resulting `ApiMessage`(s), or merge
/// tool-call blocks into a trailing assistant message. No-op for deleted /
/// detached / fully-empty messages.
fn push_converted_message(api_messages: &mut Vec<ApiMessage>, msg: &Message, ctx: &MsgConvertCtx<'_>) {
    if msg.status == MsgStatus::Deleted || msg.status == MsgStatus::Detached {
        return;
    }
    if msg.content.is_empty() && msg.tool_uses.is_empty() && msg.tool_results.is_empty() {
        return;
    }

    if msg.msg_type == MsgKind::ToolResult {
        let blocks: Vec<ContentBlock> = msg
            .tool_results
            .iter()
            .map(|result| ContentBlock::ToolResult {
                tool_use_id: result.tool_use_id.clone(),
                content: result.content.clone(),
            })
            .collect();
        if !blocks.is_empty() {
            api_messages.push(ApiMessage { role: "user".to_owned(), content: blocks });
        }
        return;
    }

    let content_blocks = if msg.msg_type == MsgKind::ToolCall {
        let Some(blocks) = build_tool_call_blocks(msg, ctx.all, ctx.idx) else { return };
        if let Some(last_api_msg) = api_messages.last_mut()
            && last_api_msg.role == "assistant"
        {
            last_api_msg.content.extend(blocks);
            return;
        }
        blocks
    } else {
        build_text_message_blocks(msg, ctx)
    };

    if !content_blocks.is_empty() {
        api_messages.push(ApiMessage { role: msg.role.clone(), content: content_blocks });
    }
}

/// Build the content blocks for a plain text message: its text plus, for the
/// last assistant turn when requested, its tool-use blocks.
fn build_text_message_blocks(msg: &Message, ctx: &MsgConvertCtx<'_>) -> Vec<ContentBlock> {
    let mut content_blocks: Vec<ContentBlock> = Vec::new();
    if !msg.content.is_empty() {
        content_blocks.push(ContentBlock::Text { text: msg.content.clone() });
    }
    let is_last = ctx.idx == ctx.all.len().saturating_sub(1);
    if msg.role == "assistant" && ctx.include_last_tool_uses && is_last && !msg.tool_uses.is_empty() {
        for tool_use in &msg.tool_uses {
            content_blocks.push(tool_use_block(tool_use));
        }
    }
    content_blocks
}

/// Context needed for panel injection into the prompt.
struct PanelInjection<'ctx> {
    /// Prepared panel messages to inject
    fake_panels: &'ctx [crate::llms::FakePanelMessage],
    /// Current time in milliseconds since UNIX epoch
    current_ms: u64,
    /// Seed/system content to re-inject after panels
    seed_content: Option<&'ctx str>,
}

/// Inject context panels as fake tool call/result message pairs.
fn inject_panel_messages(api_messages: &mut Vec<ApiMessage>, ctx: &PanelInjection<'_>) {
    for (idx, panel) in ctx.fake_panels.iter().enumerate() {
        let timestamp_text = panel_timestamp_text(panel.timestamp_ms);
        let text = if idx == 0 { format!("{}\n\n{}", panel_header_text(), timestamp_text) } else { timestamp_text };

        api_messages.push(ApiMessage {
            role: "assistant".to_owned(),
            content: vec![
                ContentBlock::Text { text },
                ContentBlock::ToolUse {
                    id: format!("panel_{}", panel.panel_id),
                    name: "dynamic_panel".to_owned(),
                    input: serde_json::json!({ "id": panel.panel_id }),
                },
            ],
        });
        api_messages.push(ApiMessage {
            role: "user".to_owned(),
            content: vec![ContentBlock::ToolResult {
                tool_use_id: format!("panel_{}", panel.panel_id),
                content: panel.content.clone(),
            }],
        });
    }

    // Footer after all panels
    let footer = panel_footer_text(ctx.current_ms);
    api_messages.push(ApiMessage {
        role: "assistant".to_owned(),
        content: vec![
            ContentBlock::Text { text: footer },
            ContentBlock::ToolUse {
                id: "panel_footer".to_owned(),
                name: "dynamic_panel".to_owned(),
                input: serde_json::json!({ "action": "end_panels" }),
            },
        ],
    });
    api_messages.push(ApiMessage {
        role: "user".to_owned(),
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "panel_footer".to_owned(),
            content: crate::infra::constants::prompts::panel_footer_ack().to_owned(),
        }],
    });

    // Re-inject seed/system prompt after panels
    if let Some(seed) = ctx.seed_content {
        api_messages.push(ApiMessage {
            role: "user".to_owned(),
            content: vec![ContentBlock::Text {
                text: format!("System instructions (repeated for emphasis):\n\n{seed}"),
            }],
        });
        api_messages.push(ApiMessage {
            role: "assistant".to_owned(),
            content: vec![ContentBlock::Text { text: "Understood. I will follow these instructions.".to_owned() }],
        });
    }
}

/// Build `ContentBlocks` for a `ToolCall` message, if it has a matching `ToolResult`.
fn build_tool_call_blocks(msg: &Message, messages: &[Message], idx: usize) -> Option<Vec<ContentBlock>> {
    let tool_use_ids: Vec<&str> = msg.tool_uses.iter().map(|t| t.id.as_str()).collect();

    let remaining = messages.get(idx.saturating_add(1)..).unwrap_or_default();
    let has_matching_result = remaining
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
    let input = if tool_use.input.is_null() { Value::Object(serde_json::Map::new()) } else { tool_use.input.clone() };
    ContentBlock::ToolUse { id: tool_use.id.clone(), name: tool_use.name.clone(), input }
}
