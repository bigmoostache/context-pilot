//! Conversation history detachment — splits old messages into frozen panels.
//!
//! Extracted from `context.rs` to keep file sizes manageable.

use crate::app::panels::now_ms;
use crate::infra::constants::{
    DETACH_CHUNK_MIN_MESSAGES, DETACH_CHUNK_MIN_TOKENS, DETACH_KEEP_MIN_MESSAGES, DETACH_KEEP_MIN_TOKENS,
};
use crate::modules::conversation::refresh::estimate_message_tokens;
use crate::state::{
    ContextElement, ContextType, Message, MessageStatus, MessageType, compute_total_pages, estimate_tokens,
};

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
pub(super) fn detach_conversation_chunks(state: &mut crate::state::State) {
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
            .map(estimate_message_tokens)
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
            tokens_seen += estimate_message_tokens(msg);

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
            .map(estimate_message_tokens)
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

        // 6. Use current time as last_refresh_ms so the history panel sorts
        //    to the end of the context. This preserves prompt cache hits for
        //    all panels before it — history panels stack progressively like
        //    icebergs calving off, instead of sinking deep and invalidating cache.
        let chunk_timestamp = now_ms();

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
            context_type: ContextType::new(ContextType::CONVERSATION_HISTORY),
            name: chunk_name,
            token_count,
            metadata: std::collections::HashMap::new(),
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
                crate::state::persistence::delete_message(uid);
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
