use crate::modules::spine::types::NotificationType;
use crate::persistence::{delete_message, save_message};
use crate::state::{ContextType, Message, PromptItem, State, estimate_tokens};

use super::ActionResult;
use super::helpers::{find_context_by_id, parse_context_pattern};

/// Handle InputSubmit action — context switching, message creation, stream start
pub fn handle_input_submit(state: &mut State) -> ActionResult {
    if state.input.is_empty() {
        return ActionResult::Nothing;
    }

    // Context switching is always allowed, even during streaming
    if let Some(id) = parse_context_pattern(&state.input)
        && let Some(index) = find_context_by_id(state, &id)
    {
        state.selected_context = index;
        state.scroll_offset = 0.0;
        state.user_scrolled = false;
        state.input.clear();
        state.input_cursor = 0;
        return ActionResult::Nothing;
    }

    let content = replace_commands(&state.input, &state.commands);
    // Expand paste sentinels: replace \x00{idx}\x00 with actual paste buffer content
    let content = expand_paste_sentinels(&content, &state.paste_buffers);
    state.input.clear();
    state.input_cursor = 0;
    state.paste_buffers.clear();
    state.paste_buffer_labels.clear();
    let user_token_estimate = estimate_tokens(&content);

    // Assign user display ID and UID
    let user_id = format!("U{}", state.next_user_id);
    let user_uid = format!("UID_{}_U", state.global_next_uid);
    state.next_user_id += 1;
    state.global_next_uid += 1;

    // Capture info for notification before moving user_msg
    let user_id_str = user_id.clone();
    let content_preview = if content.len() > 80 {
        format!("{}...", &content[..content.floor_char_boundary(80)])
    } else {
        content.clone()
    };

    let user_msg = Message::new_user(user_id, user_uid, content, user_token_estimate);
    save_message(&user_msg);

    // Add user message tokens to Conversation context and update timestamp
    if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == ContextType::Conversation) {
        ctx.token_count += user_token_estimate;
        ctx.last_refresh_ms = crate::core::panels::now_ms();
    }

    // Create a UserMessage notification — spine will detect this and start streaming
    // This works both during streaming (missed-message scenario) and when idle
    create_user_notification(state, &user_id_str, &content_preview);

    // Any human input resets auto-continuation counters — human is back in the loop
    state.spine_config.auto_continuation_count = 0;
    state.spine_config.autonomous_start_ms = None;
    state.spine_config.user_stopped = false;

    // During streaming: insert BEFORE the streaming assistant message
    // The notification will be picked up when the current stream ends
    if state.is_streaming {
        let insert_pos = state.messages.len().saturating_sub(1);
        state.messages.insert(insert_pos, user_msg);
        return ActionResult::Save;
    }

    state.messages.push(user_msg);

    // Reset per-stream and per-tick token counters for new user-initiated stream
    state.stream_cache_hit_tokens = 0;
    state.stream_cache_miss_tokens = 0;
    state.stream_output_tokens = 0;
    state.tick_cache_hit_tokens = 0;
    state.tick_cache_miss_tokens = 0;
    state.tick_output_tokens = 0;

    // Return Save — the spine check in handle_action will detect the unprocessed
    // notification and start streaming synchronously for responsive feel.
    ActionResult::Save
}

/// Handle ClearConversation action
pub fn handle_clear_conversation(state: &mut State) -> ActionResult {
    for msg in &state.messages {
        // Delete by UID if available, otherwise by id
        let file_id = msg.uid.as_ref().unwrap_or(&msg.id);
        delete_message(file_id);
    }
    state.messages.clear();
    state.input.clear();
    // Reset token count for Conversation context and update timestamp
    if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == ContextType::Conversation) {
        ctx.token_count = 0;
        ctx.last_refresh_ms = crate::core::panels::now_ms();
    }
    ActionResult::Save
}

/// Create a UserMessage notification in the spine system.
/// This is the primary trigger for starting a stream — the spine engine
/// will detect the unprocessed notification and launch streaming.
fn create_user_notification(state: &mut State, user_id: &str, content_preview: &str) {
    state.create_notification(NotificationType::UserMessage, user_id.to_string(), content_preview.to_string());
}

/// Expand paste sentinel markers (\x00{idx}\x00) with actual paste buffer content.
fn expand_paste_sentinels(input: &str, paste_buffers: &[String]) -> String {
    if !input.contains('\x00') {
        return input.to_string();
    }

    let mut result = String::new();
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == 0 {
            // Found sentinel start — find the index and closing \x00
            let start = i;
            i += 1;
            let idx_start = i;
            while i < bytes.len() && bytes[i] != 0 {
                i += 1;
            }
            if i < bytes.len() {
                // Found closing \x00
                let idx_str = &input[idx_start..i];
                i += 1; // skip closing \x00

                if let Ok(idx) = idx_str.parse::<usize>() {
                    if let Some(content) = paste_buffers.get(idx) {
                        result.push_str(content);
                    }
                    // If index out of bounds, silently drop the sentinel
                } else {
                    // Invalid index — keep original bytes
                    result.push_str(&input[start..i]);
                }
            } else {
                // No closing \x00 — keep as-is
                result.push_str(&input[start..]);
            }
        } else {
            let ch = input[i..].chars().next().unwrap();
            result.push(ch);
            i += ch.len_utf8();
        }
    }

    result
}

/// Replace /command-name tokens in input with command content.
/// Only replaces at line start (after optional whitespace).
fn replace_commands(input: &str, commands: &[PromptItem]) -> String {
    if commands.is_empty() || !input.contains('/') {
        return input.to_string();
    }

    input
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if !trimmed.starts_with('/') {
                return line.to_string();
            }
            // Extract the command token after /
            let token = &trimmed[1..];
            let (cmd_id, rest) = match token.find(|c: char| c.is_whitespace()) {
                Some(pos) => (&token[..pos], &token[pos..]),
                None => (token, ""),
            };
            if let Some(cmd) = commands.iter().find(|c| c.id == cmd_id) {
                format!("{}{}", cmd.content.trim_end(), rest)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
