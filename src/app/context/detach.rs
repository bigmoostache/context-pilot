//! Conversation history detachment — splits old messages into frozen panels.
//!
//! Extracted from `context.rs` to keep file sizes manageable.

use crate::app::panels::now_ms;
use crate::infra::constants::{
    DETACH_CHUNK_MIN_MESSAGES, DETACH_CHUNK_MIN_TOKENS, DETACH_KEEP_MIN_MESSAGES, DETACH_KEEP_MIN_TOKENS,
};
use crate::modules::conversation::refresh::estimate_message_tokens;
use crate::state::cache::hash_content;
use crate::state::{Entry, Kind, Message, MsgKind, MsgStatus, compute_total_pages, estimate_tokens};
use cp_base::panels::time_arith;

/// Check if `idx` is a turn boundary — a safe place to split the conversation.
/// A turn boundary is after a complete assistant turn:
/// - After an assistant text message (not a tool call)
/// - After a tool result, IF the next message is a user text message (end of tool loop)
/// - After a tool result that is the last message (shouldn't happen but handle gracefully)
fn is_turn_boundary(messages: &[Message], idx: usize) -> bool {
    let Some(msg) = messages.get(idx) else {
        return false;
    };

    // Skip Deleted/Detached messages — not meaningful boundaries
    if msg.status == MsgStatus::Deleted || msg.status == MsgStatus::Detached {
        return false;
    }

    // After an assistant text message (not a tool call)
    if msg.role == "assistant" && msg.msg_type == MsgKind::TextMessage {
        return true;
    }

    // After a tool result, if next non-skipped message is a user text message
    if msg.msg_type == MsgKind::ToolResult {
        let rest = messages.get(idx.saturating_add(1)..).unwrap_or_default();
        for next in rest {
            if next.status == MsgStatus::Deleted || next.status == MsgStatus::Detached {
                continue;
            }
            return next.role == "user" && next.msg_type == MsgKind::TextMessage;
        }
        return true; // Last message in conversation
    }

    false
}

/// Format a range of messages into a text chunk (delegates to shared function).
fn format_chunk_content(messages: &[Message], start: usize, end: usize) -> String {
    let slice = messages.get(start..end).unwrap_or_default();
    crate::state::format_messages_to_chunk(slice)
}

/// Detach oldest conversation messages into frozen `ConversationHistory` panels
/// when the active conversation exceeds thresholds.
///
/// All four constraints must be met to detach:
/// 1. Chunk has >= `DETACH_CHUNK_MIN_MESSAGES` active messages
/// 2. Chunk has >= `DETACH_CHUNK_MIN_TOKENS` estimated tokens
/// 3. Remaining tip keeps >= `DETACH_KEEP_MIN_MESSAGES` active messages
/// 4. Remaining tip keeps >= `DETACH_KEEP_MIN_TOKENS` estimated tokens
pub(super) fn detach_conversation_chunks(state: &mut crate::state::State) {
    loop {
        // 1. Count active (non-Deleted, non-Detached) messages and total tokens
        let active_count =
            state.messages.iter().filter(|m| m.status != MsgStatus::Deleted && m.status != MsgStatus::Detached).count();
        let total_tokens: usize = state
            .messages
            .iter()
            .filter(|m| m.status != MsgStatus::Deleted && m.status != MsgStatus::Detached)
            .map(estimate_message_tokens)
            .sum();

        // 2. Quick check: if we can't possibly satisfy both chunk minimums
        //    while leaving enough in the tip, bail early.
        if active_count < DETACH_CHUNK_MIN_MESSAGES.saturating_add(DETACH_KEEP_MIN_MESSAGES) {
            break;
        }
        if total_tokens < DETACH_CHUNK_MIN_TOKENS.saturating_add(DETACH_KEEP_MIN_TOKENS) {
            break;
        }

        // 3. Walk from oldest, tracking both message count and token count.
        //    Only consider a boundary once BOTH chunk minimums are reached.
        let mut active_seen = 0usize;
        let mut tokens_seen = 0usize;
        let mut boundary = None;

        for (idx, msg) in state.messages.iter().enumerate() {
            if msg.status == MsgStatus::Deleted || msg.status == MsgStatus::Detached {
                continue;
            }
            active_seen = active_seen.saturating_add(1);
            tokens_seen = tokens_seen.saturating_add(estimate_message_tokens(msg));

            if active_seen >= DETACH_CHUNK_MIN_MESSAGES
                && tokens_seen >= DETACH_CHUNK_MIN_TOKENS
                && is_turn_boundary(&state.messages, idx)
            {
                boundary = Some(idx.saturating_add(1)); // exclusive end
                break;
            }
        }

        let boundary = match boundary {
            Some(b) if b > 0 => b,
            _ => break, // No valid boundary found, bail
        };

        // 4. Verify the remaining tip satisfies both keep minimums
        let remaining_msgs = state.messages.get(boundary..).unwrap_or_default();
        let remaining_active =
            remaining_msgs.iter().filter(|m| m.status != MsgStatus::Deleted && m.status != MsgStatus::Detached).count();
        let remaining_tokens: usize = remaining_msgs
            .iter()
            .filter(|m| m.status != MsgStatus::Deleted && m.status != MsgStatus::Detached)
            .map(estimate_message_tokens)
            .sum();

        if remaining_active < DETACH_KEEP_MIN_MESSAGES || remaining_tokens < DETACH_KEEP_MIN_TOKENS {
            break;
        }

        // 4. Collect message IDs for the chunk name
        let chunk_msgs = state.messages.get(..boundary).unwrap_or_default();
        let first_timestamp = chunk_msgs
            .iter()
            .find(|m| m.status != MsgStatus::Deleted && m.status != MsgStatus::Detached)
            .map_or(0, |m| m.timestamp_ms);
        let last_timestamp = chunk_msgs
            .iter()
            .rev()
            .find(|m| m.status != MsgStatus::Deleted && m.status != MsgStatus::Detached)
            .map_or(0, |m| m.timestamp_ms);

        // 5. Collect Message objects for UI rendering + format chunk content for LLM
        let history_msgs: Vec<Message> = chunk_msgs
            .iter()
            .filter(|m| m.status != MsgStatus::Deleted && m.status != MsgStatus::Detached)
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
                let secs = time_arith::ms_to_secs(ms);
                let (hours, minutes, _seconds) = time_arith::secs_to_hms(secs);
                format!("{hours:02}:{minutes:02}")
            }
            if first_timestamp > 0 && last_timestamp > 0 {
                format!("Chat {}–{}", ms_to_short_time(first_timestamp), ms_to_short_time(last_timestamp))
            } else {
                format!("Chat ({active_seen})")
            }
        };

        let panel_global_uid = format!("UID_{}_P", state.global_next_uid);
        state.global_next_uid = state.global_next_uid.saturating_add(1);

        state.context.push(Entry {
            id: panel_id,
            uid: Some(panel_global_uid),
            context_type: Kind::new(Kind::CONVERSATION_HISTORY),
            name: chunk_name,
            token_count,
            metadata: std::collections::HashMap::new(),
            cached_content: Some(content.clone()),
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
            freeze_count: 0,
            total_freezes: 0,
            total_cache_misses: 0,
            last_emitted_hash: Some(hash_content(&content)),
            last_emitted_content: Some(content),
            last_emitted_context: None,
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
