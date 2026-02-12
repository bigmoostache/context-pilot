use crate::core::panels::{collect_all_context, now_ms, refresh_all_panels, ContextItem};
use crate::constants::{DETACH_CHUNK_MESSAGES, DETACH_KEEP_MESSAGES};
use crate::state::{
    compute_total_pages, estimate_tokens, ContextElement, ContextType,
    Message, MessageStatus, MessageType, State,
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
    let context_items = collect_all_context(state);

    // Dynamically enable/disable panel_goto_page based on whether any panel is paginated
    let has_paginated = state.context.iter().any(|c| c.total_pages > 1);
    for tool in &mut state.tools {
        if tool.id == "panel_goto_page" {
            tool.enabled = has_paginated;
        }
    }

    // Prepare messages
    let messages: Vec<_> = if include_last_message {
        state.messages.iter()
            .filter(|m| !m.content.is_empty() || !m.tool_uses.is_empty() || !m.tool_results.is_empty())
            .cloned()
            .collect()
    } else {
        state.messages.iter()
            .filter(|m| !m.content.is_empty() || !m.tool_uses.is_empty() || !m.tool_results.is_empty())
            .take(state.messages.len().saturating_sub(1))
            .cloned()
            .collect()
    };

    StreamContext {
        messages,
        context_items,
        tools: state.tools.clone(),
    }
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

/// Format a range of messages into a text chunk for the ConversationHistory panel.
/// Uses the same `[ID]:\ncontent` format the LLM sees in conversation messages.
fn format_chunk_content(messages: &[Message], start: usize, end: usize) -> String {
    let mut output = String::new();
    for msg in &messages[start..end] {
        if msg.status == MessageStatus::Deleted || msg.status == MessageStatus::Detached {
            continue;
        }
        match msg.message_type {
            MessageType::ToolCall => {
                for tu in &msg.tool_uses {
                    output += &format!(
                        "tool_call {}({})\n",
                        tu.name,
                        serde_json::to_string(&tu.input).unwrap_or_default()
                    );
                }
            }
            MessageType::ToolResult => {
                for tr in &msg.tool_results {
                    output += &format!("{}\n", tr.content);
                }
            }
            MessageType::TextMessage => {
                let content = match msg.status {
                    MessageStatus::Summarized => {
                        msg.tl_dr.as_deref().unwrap_or(&msg.content)
                    }
                    _ => &msg.content,
                };
                if !content.is_empty() {
                    output += &format!("[{}]: {}\n", msg.role, content);
                }
            }
        }
    }
    output
}

/// Detach oldest conversation messages into frozen ConversationHistory panels
/// when the active message count exceeds DETACH_CHUNK_MESSAGES + DETACH_KEEP_MESSAGES.
pub fn detach_conversation_chunks(state: &mut State) {
    loop {
        // 1. Count active (non-Deleted, non-Detached) messages
        let active_count = state.messages.iter()
            .filter(|m| m.status != MessageStatus::Deleted && m.status != MessageStatus::Detached)
            .count();

        // 2. If within limits, nothing to detach
        if active_count <= DETACH_CHUNK_MESSAGES + DETACH_KEEP_MESSAGES {
            break;
        }

        // 3. Walk from oldest, count DETACH_CHUNK_MESSAGES active messages,
        //    then snap forward to the nearest turn boundary.
        let mut active_seen = 0usize;
        let mut boundary = None;

        for (idx, msg) in state.messages.iter().enumerate() {
            if msg.status == MessageStatus::Deleted || msg.status == MessageStatus::Detached {
                continue;
            }
            active_seen += 1;

            if active_seen >= DETACH_CHUNK_MESSAGES && is_turn_boundary(&state.messages, idx) {
                boundary = Some(idx + 1); // exclusive end
                break;
            }
        }

        let boundary = match boundary {
            Some(b) if b > 0 => b,
            _ => break, // No valid boundary found, bail
        };

        // 3b. Safety check: ensure enough active messages remain after detachment
        let remaining_active = state.messages[boundary..].iter()
            .filter(|m| m.status != MessageStatus::Deleted && m.status != MessageStatus::Detached)
            .count();
        if remaining_active < DETACH_KEEP_MESSAGES {
            break;
        }

        // 4. Collect message IDs for the chunk name
        let first_timestamp = state.messages[..boundary].iter()
            .find(|m| m.status != MessageStatus::Deleted && m.status != MessageStatus::Detached)
            .map(|m| m.timestamp_ms)
            .unwrap_or(0);
        let last_timestamp = state.messages[..boundary].iter().rev()
            .find(|m| m.status != MessageStatus::Deleted && m.status != MessageStatus::Detached)
            .map(|m| m.timestamp_ms)
            .unwrap_or(0);

        // 5. Collect Message objects for UI rendering + format chunk content for LLM
        let history_msgs: Vec<Message> = state.messages[..boundary].iter()
            .filter(|m| m.status != MessageStatus::Deleted && m.status != MessageStatus::Detached)
            .cloned()
            .collect();

        let content = format_chunk_content(&state.messages, 0, boundary);
        if content.is_empty() {
            break; // Nothing useful to detach
        }

        // 6. Get timestamp from first active message (for sort ordering — oldest first)
        let chunk_timestamp = state.messages[..boundary].iter()
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
            file_hash: None,
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
            result_command_hash: None,
            skill_prompt_id: None,
            cached_content: Some(content),
            history_messages: Some(history_msgs),
            cache_deprecated: false,
            cache_in_flight: false,
            last_refresh_ms: chunk_timestamp,
            content_hash: None,
            tmux_last_lines_hash: None,
            current_page: 0,
            total_pages,
            full_token_count: token_count,
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
