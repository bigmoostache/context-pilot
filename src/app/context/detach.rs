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
///
/// A boundary is valid ONLY when the next active message is a user
/// `TextMessage` (a fresh user turn). This guarantees the remaining
/// conversation never starts with an orphaned `ToolResult` whose
/// matching `ToolCall` was detached into a history panel.
fn is_turn_boundary(messages: &[Message], idx: usize) -> bool {
    let Some(msg) = messages.get(idx) else {
        return false;
    };

    // Deleted/Detached messages are not meaningful boundaries
    if msg.status == MsgStatus::Deleted || msg.status == MsgStatus::Detached {
        return false;
    }

    // Walk forward to the next active message — split is safe only if
    // it's a user TextMessage (new human turn, clean slate).
    let rest = messages.get(idx.saturating_add(1)..).unwrap_or_default();
    for next in rest {
        if next.status == MsgStatus::Deleted || next.status == MsgStatus::Detached {
            continue;
        }
        return next.role == "user" && next.msg_type == MsgKind::TextMessage;
    }
    // Last active message in conversation — safe to detach everything
    true
}

/// Format a range of messages into a text chunk (delegates to shared function).
fn format_chunk_content(messages: &[Message], start: usize, end: usize) -> String {
    let slice = messages.get(start..end).unwrap_or_default();
    crate::state::format_messages_to_chunk(slice)
}

/// Count active (non-Deleted, non-Detached) messages in `msgs`.
fn active_count(msgs: &[Message]) -> usize {
    msgs.iter().filter(|m| m.status != MsgStatus::Deleted && m.status != MsgStatus::Detached).count()
}

/// Sum estimated tokens over active messages in `msgs`.
fn active_tokens(msgs: &[Message]) -> usize {
    msgs.iter()
        .filter(|m| m.status != MsgStatus::Deleted && m.status != MsgStatus::Detached)
        .map(estimate_message_tokens)
        .sum()
}

/// Walk from oldest, returning the exclusive-end index of the first detachable
/// chunk: a turn boundary reached once BOTH chunk minimums (messages + tokens)
/// are satisfied. `None` when no such boundary exists.
fn find_detach_boundary(messages: &[Message]) -> Option<usize> {
    let mut active_seen = 0usize;
    let mut tokens_seen = 0usize;
    for (idx, msg) in messages.iter().enumerate() {
        if msg.status == MsgStatus::Deleted || msg.status == MsgStatus::Detached {
            continue;
        }
        active_seen = active_seen.saturating_add(1);
        tokens_seen = tokens_seen.saturating_add(estimate_message_tokens(msg));

        if active_seen >= DETACH_CHUNK_MIN_MESSAGES
            && tokens_seen >= DETACH_CHUNK_MIN_TOKENS
            && is_turn_boundary(messages, idx)
        {
            return (idx.saturating_add(1) > 0).then(|| idx.saturating_add(1));
        }
    }
    None
}

/// Whether the tip remaining after `boundary` still satisfies both keep minimums.
fn tip_keeps_enough(messages: &[Message], boundary: usize) -> bool {
    let remaining = messages.get(boundary..).unwrap_or_default();
    active_count(remaining) >= DETACH_KEEP_MIN_MESSAGES && active_tokens(remaining) >= DETACH_KEEP_MIN_TOKENS
}

/// Build the `Chat HH:MM–HH:MM` (or `Chat (N)`) name for a detached chunk.
fn chunk_name(first_ts: u64, last_ts: u64, active_seen: usize) -> String {
    fn ms_to_short_time(ms: u64) -> String {
        let secs = time_arith::ms_to_secs(ms);
        let (hours, minutes, _seconds) = time_arith::secs_to_hms(secs);
        format!("{hours:02}:{minutes:02}")
    }
    if first_ts > 0 && last_ts > 0 {
        format!("Chat {}–{}", ms_to_short_time(first_ts), ms_to_short_time(last_ts))
    } else {
        format!("Chat ({active_seen})")
    }
}

/// Create the `ConversationHistory` panel for `messages[..boundary]` and push it
/// onto `state.context`. Returns `false` when the chunk content is empty (bail).
fn push_history_chunk(state: &mut crate::state::State, boundary: usize) -> bool {
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
    let seen = active_count(chunk_msgs);

    let history_msgs: Vec<Message> = chunk_msgs
        .iter()
        .filter(|m| m.status != MsgStatus::Deleted && m.status != MsgStatus::Detached)
        .cloned()
        .collect();

    let content = format_chunk_content(&state.messages, 0, boundary);
    if content.is_empty() {
        return false;
    }

    // Use current time as last_refresh_ms so the history panel sorts to the end
    // of the context, preserving prompt cache hits for all panels before it —
    // history panels stack progressively like icebergs calving off.
    let chunk_timestamp = now_ms();
    let panel_id = state.next_available_context_id();
    let token_count = estimate_tokens(&content);
    let total_pages = compute_total_pages(token_count);
    let name = chunk_name(first_timestamp, last_timestamp, seen);

    let panel_global_uid = format!("UID_{}_P", state.global_next_uid);
    state.global_next_uid = state.global_next_uid.saturating_add(1);

    state.context.push(Entry {
        id: panel_id,
        uid: Some(panel_global_uid),
        context_type: Kind::new(Kind::CONVERSATION_HISTORY),
        name,
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
        page_descriptions: std::collections::BTreeMap::new(),
        full_token_count: token_count,
        scroll_state: cp_base::state::context::ScrollState::default(),
        panel_cache_hit: false,
        panel_total_cost: 0.0,
        freeze_count: 0,
        total_freezes: 0,
        total_cache_misses: 0,
        emitted: cp_base::state::context::EmittedState { hash: Some(hash_content(&content)), context: None },
    });

    // Remove detached messages from state and disk.
    let removed: Vec<Message> = state.messages.drain(..boundary).collect();
    for msg in &removed {
        if let Some(uid) = &msg.uid {
            crate::state::persistence::delete_message(uid);
        }
    }
    true
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
    let _fg = cp_base::flame!("detach");
    // Don't detach while context is frozen — detaching would invalidate
    // the cached prompt prefix that freezing is trying to preserve.
    if cp_mod_queue::types::QueueState::get(state).active || state.tempo {
        return;
    }

    loop {
        // Quick check: bail if we can't satisfy both chunk minimums while
        // leaving enough in the tip.
        if active_count(&state.messages) < DETACH_CHUNK_MIN_MESSAGES.saturating_add(DETACH_KEEP_MIN_MESSAGES) {
            break;
        }
        if active_tokens(&state.messages) < DETACH_CHUNK_MIN_TOKENS.saturating_add(DETACH_KEEP_MIN_TOKENS) {
            break;
        }

        let Some(boundary) = find_detach_boundary(&state.messages) else { break };
        if !tip_keeps_enough(&state.messages, boundary) {
            break;
        }
        if !push_history_chunk(state, boundary) {
            break;
        }
        // Loop to check if remaining messages still exceed threshold.
    }
}
