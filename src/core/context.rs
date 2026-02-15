use crate::cache::hash_content;
use crate::constants::{
    DETACH_CHUNK_MIN_MESSAGES, DETACH_CHUNK_MIN_TOKENS, DETACH_KEEP_MIN_MESSAGES, DETACH_KEEP_MIN_TOKENS,
};
use crate::core::panels::{ContextItem, collect_all_context, now_ms, refresh_all_panels};
use crate::state::{
    ContextElement, ContextType, Message, MessageStatus, MessageType, State, compute_total_pages, estimate_tokens,
};
use crate::tool_defs::ToolDefinition;
use crate::tools::refresh_conversation_context;

/// Context data prepared for streaming
pub struct StreamContext {
    pub messages: Vec<Message>,
    pub context_items: Vec<ContextItem>,
    pub tools: Vec<ToolDefinition>,
}

/// Refresh all context elements and prepare data for streaming.
///
/// Every call to this function means the LLM is about to see the full
/// conversation history (including any user messages that arrived during
/// streaming). We therefore mark all UserMessage notifications as processed
/// here — the LLM has "seen" them via the rebuilt context.
pub fn prepare_stream_context(state: &mut State, include_last_message: bool) -> StreamContext {
    // Mark UserMessage notifications as processed on every context rebuild.
    // This prevents the spine from firing a redundant auto-continuation for
    // messages the LLM already saw (e.g., user sent a message during a tool
    // call pause — the message is in context, LLM responds, but without this
    // the notification would still be "unprocessed" when the stream ends).
    state.mark_user_message_notifications_processed();

    // Detach old conversation chunks before anything else
    detach_conversation_chunks(state);

    // Refresh conversation token counts (not panel-based yet)
    refresh_conversation_context(state);

    // Refresh all panel token counts
    refresh_all_panels(state);

    // Collect all context items from panels
    let mut context_items = collect_all_context(state);

    // Sort panels by last_refresh_ms ascending (oldest first, newest closest
    // to conversation). This ordering determines prompt caching: the LLM
    // provider sees panels in this order, and Anthropic-style prefix caching
    // means earlier panels are more likely to be cache hits.
    context_items.sort_by_key(|item| item.last_refresh_ms);

    // === Panel cache cost tracking ===
    // Hash each panel's content (what the LLM literally sees), build an ordered
    // hash list, compare to previous tick's list via prefix matching, and
    // accumulate per-panel costs based on cache hit/miss pricing.
    {
        // Build hash list from panel content (excluding "chat" which is conversation)
        let panel_hashes: Vec<(String, String, usize)> = context_items
            .iter()
            .filter(|item| item.id != "chat")
            .map(|item| {
                let h = hash_content(&item.content);
                (item.id.clone(), h, estimate_tokens(&item.content))
            })
            .collect();

        let new_hash_list: Vec<String> = panel_hashes.iter().map(|(id, h, _)| format!("{}:{}", id, h)).collect();

        // Find max prefix match index
        let prev = &state.previous_panel_hash_list;
        let prefix_len = new_hash_list.iter().zip(prev.iter()).take_while(|(a, b)| a == b).count();

        // Get pricing from current model
        let hit_price = state.cache_hit_price_per_mtok();
        let miss_price = state.cache_miss_price_per_mtok();

        // Update each panel's cache hit status and accumulate cost
        for (i, (panel_id, _, token_count)) in panel_hashes.iter().enumerate() {
            let is_hit = i < prefix_len;
            let price = if is_hit { hit_price } else { miss_price };
            let cost = *token_count as f64 * price as f64 / 1_000_000.0;

            if let Some(ctx) = state.context.iter_mut().find(|c| c.id == *panel_id) {
                ctx.panel_cache_hit = is_hit;
                ctx.panel_total_cost += cost;
            }
        }

        // Store hash list for next tick
        state.previous_panel_hash_list = new_hash_list;
    }

    // Dynamically enable/disable panel_goto_page based on whether any panel is paginated
    let has_paginated = state.context.iter().any(|c| c.total_pages > 1);
    for tool in &mut state.tools {
        if tool.id == "panel_goto_page" {
            tool.enabled = has_paginated;
        }
    }

    // Prepare messages
    let messages: Vec<_> = if include_last_message {
        state
            .messages
            .iter()
            .filter(|m| !m.content.is_empty() || !m.tool_uses.is_empty() || !m.tool_results.is_empty())
            .cloned()
            .collect()
    } else {
        state
            .messages
            .iter()
            .filter(|m| !m.content.is_empty() || !m.tool_uses.is_empty() || !m.tool_results.is_empty())
            .take(state.messages.len().saturating_sub(1))
            .cloned()
            .collect()
    };

    StreamContext { messages, context_items, tools: state.tools.clone() }
}

// ─── Conversation History Detachment ────────────────────────────────────────

/// Check if `idx` is a turn boundary — a safe place to split the conversation.
/// A turn boundary is after a complete assistant turn:
/// - After an assistant text message (not a tool call)
/// - After a tool result, IF the next message is a user text message (end of tool loop)
/// - After a tool result that is the last message (shouldn't happen but handle gracefully)
fn is_turn_boundary(messages: &[Message], idx: usize) -> bool {
    let msg = &messages[idx];

    // Skip Deleted/Detached messages — not meaningful boundaries
    if msg.status == MessageStatus::Deleted || msg.status == MessageStatus::Detached {
        return false;
    }

    // After an assistant text message (not a tool call)
    if msg.role == "assistant" && msg.message_type == MessageType::TextMessage {
        return true;
    }

    // After a tool result, if next non-skipped message is a user text message
    if msg.message_type == MessageType::ToolResult {
        for next in &messages[idx + 1..] {
            if next.status == MessageStatus::Deleted || next.status == MessageStatus::Detached {
                continue;
            }
            return next.role == "user" && next.message_type == MessageType::TextMessage;
        }
        return true; // Last message in conversation
    }

    false
}

/// Format a range of messages into a text chunk (delegates to shared function).
fn format_chunk_content(messages: &[Message], start: usize, end: usize) -> String {
    crate::state::format_messages_to_chunk(&messages[start..end])
}

/// Detach oldest conversation messages into frozen ConversationHistory panels
/// when the active conversation exceeds thresholds.
///
/// All four constraints must be met to detach:
/// 1. Chunk has >= DETACH_CHUNK_MIN_MESSAGES active messages
/// 2. Chunk has >= DETACH_CHUNK_MIN_TOKENS estimated tokens
/// 3. Remaining tip keeps >= DETACH_KEEP_MIN_MESSAGES active messages
/// 4. Remaining tip keeps >= DETACH_KEEP_MIN_TOKENS estimated tokens
pub fn detach_conversation_chunks(state: &mut State) {
    loop {
        // 1. Count active (non-Deleted, non-Detached) messages and total tokens
        let active_count = state
            .messages
            .iter()
            .filter(|m| m.status != MessageStatus::Deleted && m.status != MessageStatus::Detached)
            .count();
        let total_tokens: usize = state
            .messages
            .iter()
            .filter(|m| m.status != MessageStatus::Deleted && m.status != MessageStatus::Detached)
            .map(|m| estimate_tokens(&m.content))
            .sum();

        // 2. Quick check: if we can't possibly satisfy both chunk minimums
        //    while leaving enough in the tip, bail early.
        if active_count < DETACH_CHUNK_MIN_MESSAGES + DETACH_KEEP_MIN_MESSAGES {
            break;
        }
        if total_tokens < DETACH_CHUNK_MIN_TOKENS + DETACH_KEEP_MIN_TOKENS {
            break;
        }

        // 3. Walk from oldest, tracking both message count and token count.
        //    Only consider a boundary once BOTH chunk minimums are reached.
        let mut active_seen = 0usize;
        let mut tokens_seen = 0usize;
        let mut boundary = None;

        for (idx, msg) in state.messages.iter().enumerate() {
            if msg.status == MessageStatus::Deleted || msg.status == MessageStatus::Detached {
                continue;
            }
            active_seen += 1;
            tokens_seen += estimate_tokens(&msg.content);

            if active_seen >= DETACH_CHUNK_MIN_MESSAGES
                && tokens_seen >= DETACH_CHUNK_MIN_TOKENS
                && is_turn_boundary(&state.messages, idx)
            {
                boundary = Some(idx + 1); // exclusive end
                break;
            }
        }

        let boundary = match boundary {
            Some(b) if b > 0 => b,
            _ => break, // No valid boundary found, bail
        };

        // 4. Verify the remaining tip satisfies both keep minimums
        let remaining_active = state.messages[boundary..]
            .iter()
            .filter(|m| m.status != MessageStatus::Deleted && m.status != MessageStatus::Detached)
            .count();
        let remaining_tokens: usize = state.messages[boundary..]
            .iter()
            .filter(|m| m.status != MessageStatus::Deleted && m.status != MessageStatus::Detached)
            .map(|m| estimate_tokens(&m.content))
            .sum();

        if remaining_active < DETACH_KEEP_MIN_MESSAGES || remaining_tokens < DETACH_KEEP_MIN_TOKENS {
            break;
        }

        // 4. Collect message IDs for the chunk name
        let first_timestamp = state.messages[..boundary]
            .iter()
            .find(|m| m.status != MessageStatus::Deleted && m.status != MessageStatus::Detached)
            .map(|m| m.timestamp_ms)
            .unwrap_or(0);
        let last_timestamp = state.messages[..boundary]
            .iter()
            .rev()
            .find(|m| m.status != MessageStatus::Deleted && m.status != MessageStatus::Detached)
            .map(|m| m.timestamp_ms)
            .unwrap_or(0);

        // 5. Collect Message objects for UI rendering + format chunk content for LLM
        let history_msgs: Vec<Message> = state.messages[..boundary]
            .iter()
            .filter(|m| m.status != MessageStatus::Deleted && m.status != MessageStatus::Detached)
            .cloned()
            .collect();

        let content = format_chunk_content(&state.messages, 0, boundary);
        if content.is_empty() {
            break; // Nothing useful to detach
        }

        // 6. Get timestamp from first active message (for sort ordering — oldest first)
        let chunk_timestamp = state.messages[..boundary]
            .iter()
            .find(|m| m.status != MessageStatus::Deleted && m.status != MessageStatus::Detached)
            .map(|m| m.timestamp_ms)
            .unwrap_or_else(now_ms);

        // 7. Create the ConversationHistory panel
        let panel_id = state.next_available_context_id();
        let token_count = estimate_tokens(&content);
        let total_pages = compute_total_pages(token_count);
        let chunk_name = {
            // Format timestamps as short time strings (HH:MM)
            fn ms_to_short_time(ms: u64) -> String {
                let secs = ms / 1000;
                let hours = (secs % 86400) / 3600;
                let minutes = (secs % 3600) / 60;
                format!("{:02}:{:02}", hours, minutes)
            }
            if first_timestamp > 0 && last_timestamp > 0 {
                format!("Chat {}–{}", ms_to_short_time(first_timestamp), ms_to_short_time(last_timestamp))
            } else {
                format!("Chat ({})", active_seen)
            }
        };

        let panel_uid = format!("UID_{}_P", state.global_next_uid);
        state.global_next_uid += 1;

        state.context.push(ContextElement {
            id: panel_id,
            uid: Some(panel_uid),
            context_type: ContextType::ConversationHistory,
            name: chunk_name,
            token_count,
            file_path: None,
            glob_pattern: None,
            glob_path: None,
            grep_pattern: None,
            grep_path: None,
            grep_file_pattern: None,
            tmux_pane_id: None,
            tmux_lines: None,
            tmux_last_keys: None,
            tmux_description: None,
            result_command: None,
            skill_prompt_id: None,
            cached_content: Some(content),
            history_messages: Some(history_msgs),
            cache_deprecated: false,
            cache_in_flight: false,
            last_refresh_ms: chunk_timestamp,
            content_hash: None,
            source_hash: None,
            current_page: 0,
            total_pages,
            full_token_count: token_count,
            panel_cache_hit: false,
            panel_total_cost: 0.0,
        });

        // 8. Remove detached messages from state and disk
        let removed: Vec<Message> = state.messages.drain(..boundary).collect();
        for msg in &removed {
            if let Some(uid) = &msg.uid {
                crate::persistence::delete_message(uid);
            }
        }

        // Loop to check if remaining messages still exceed threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::message::test_helpers::MessageBuilder;

    #[test]
    fn turn_boundary_assistant_text() {
        let msgs = vec![MessageBuilder::user("hi").build(), MessageBuilder::assistant("hello").build()];
        assert!(!is_turn_boundary(&msgs, 0)); // user msg — not a boundary
        assert!(is_turn_boundary(&msgs, 1)); // assistant text — boundary
    }

    #[test]
    fn turn_boundary_tool_call_not_boundary() {
        let msgs = vec![MessageBuilder::tool_call("read_file", serde_json::json!({})).build()];
        assert!(!is_turn_boundary(&msgs, 0));
    }

    #[test]
    fn turn_boundary_tool_result_then_user() {
        let msgs = vec![MessageBuilder::tool_result("T1", "ok").build(), MessageBuilder::user("next question").build()];
        assert!(is_turn_boundary(&msgs, 0)); // tool result + next user = boundary
    }

    #[test]
    fn turn_boundary_tool_result_then_tool_call() {
        let msgs = vec![
            MessageBuilder::tool_result("T1", "ok").build(),
            MessageBuilder::tool_call("write_file", serde_json::json!({})).build(),
        ];
        assert!(!is_turn_boundary(&msgs, 0)); // next is tool call, not user — not a boundary
    }

    #[test]
    fn turn_boundary_tool_result_last_message() {
        let msgs = vec![MessageBuilder::tool_result("T1", "ok").build()];
        assert!(is_turn_boundary(&msgs, 0)); // last message — boundary
    }

    #[test]
    fn turn_boundary_deleted_not_boundary() {
        let msgs = vec![MessageBuilder::assistant("deleted").status(MessageStatus::Deleted).build()];
        assert!(!is_turn_boundary(&msgs, 0));
    }

    #[test]
    fn turn_boundary_detached_not_boundary() {
        let msgs = vec![MessageBuilder::assistant("detached").status(MessageStatus::Detached).build()];
        assert!(!is_turn_boundary(&msgs, 0));
    }

    #[test]
    fn turn_boundary_tool_result_skips_deleted_next() {
        // tool_result, then deleted, then user — should still be boundary
        let msgs = vec![
            MessageBuilder::tool_result("T1", "ok").build(),
            MessageBuilder::user("ignored").status(MessageStatus::Deleted).build(),
            MessageBuilder::user("real next").build(),
        ];
        assert!(is_turn_boundary(&msgs, 0));
    }
}
