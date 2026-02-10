//! Spine engine — the central check that evaluates auto-continuations and guard rails.
//!
//! Called from app.rs both periodically (main loop) and synchronously (after InputSubmit).

use crate::core::panels::now_ms;
use crate::state::{ContextType, State};

use super::continuation::all_continuations;
use super::guard_rail::all_guard_rails;
use super::types::{ContinuationAction, Notification, NotificationType};

/// Result of a spine check — tells the caller what to do.
#[derive(Debug)]
pub enum SpineDecision {
    /// Nothing to do — no continuation needed
    Idle,
    /// A guard rail blocked auto-continuation
    Blocked(String),
    /// An auto-continuation fired — launch a new stream
    Continue(ContinuationAction),
}

/// Evaluate the spine: check guard rails, then auto-continuations.
///
/// Returns a `SpineDecision` telling the caller what to do.
/// The caller (app.rs) is responsible for actually starting the stream.
///
/// This function does NOT start streaming itself — it just decides.
pub fn check_spine(state: &mut State) -> SpineDecision {
    // Never launch if already streaming
    if state.is_streaming {
        return SpineDecision::Idle;
    }

    // Check if any auto-continuation wants to fire
    let continuations = all_continuations();
    let mut triggered = None;
    for cont in &continuations {
        if cont.should_continue(state) {
            triggered = Some(cont);
            break;
        }
    }

    // If nothing wants to continue, we're idle
    let cont = match triggered {
        Some(c) => c,
        None => return SpineDecision::Idle,
    };

    // Something wants to continue — check guard rails
    let guard_rails = all_guard_rails();
    for guard in &guard_rails {
        if guard.should_block(state) {
            let reason = guard.block_reason(state);
            // Create a notification about the block
            let notif_id = format!("N{}", state.next_notification_id);
            state.next_notification_id += 1;
            state.notifications.push(Notification {
                id: notif_id,
                notification_type: NotificationType::Custom,
                source: format!("guard_rail:{}", guard.name()),
                processed: false,
                timestamp_ms: now_ms(),
                content: format!("Auto-continuation blocked by {}: {}", guard.name(), reason),
            });
            state.touch_panel(ContextType::Spine);
            return SpineDecision::Blocked(reason);
        }
    }

    // All guard rails passed — fire the continuation
    let action = cont.build_continuation(state);
    let cont_name = cont.name().to_string();

    // Update tracking state
    state.spine_config.auto_continuation_count += 1;
    if state.spine_config.autonomous_start_ms.is_none() {
        state.spine_config.autonomous_start_ms = Some(now_ms());
    }

    // Create a notification about the auto-continuation
    let notif_id = format!("N{}", state.next_notification_id);
    state.next_notification_id += 1;
    state.notifications.push(Notification {
        id: notif_id,
        notification_type: NotificationType::Custom,
        source: format!("auto_continue:{}", cont_name),
        processed: true, // Auto-continuation notifications are pre-processed
        timestamp_ms: now_ms(),
        content: format!("Auto-continuation fired: {}", cont_name),
    });
    state.touch_panel(ContextType::Spine);

    SpineDecision::Continue(action)
}

/// Apply a continuation action to state: create synthetic message, set up for streaming.
///
/// Returns true if a stream should be started (caller should call start_streaming).
pub fn apply_continuation(state: &mut State, action: ContinuationAction) -> bool {
    use crate::persistence::save_message;
    use crate::state::{Message, MessageStatus, MessageType, estimate_tokens};

    // Auto-mark UserMessage notifications as processed when starting a new stream
    for n in &mut state.notifications {
        if !n.processed && n.notification_type == NotificationType::UserMessage {
            n.processed = true;
        }
    }

    match action {
        ContinuationAction::SyntheticMessage(content) => {
            let user_token_estimate = estimate_tokens(&content);

            // Create synthetic user message
            let user_id = format!("U{}", state.next_user_id);
            let user_uid = format!("UID_{}_U", state.global_next_uid);
            state.next_user_id += 1;
            state.global_next_uid += 1;

            let user_msg = Message {
                id: user_id,
                uid: Some(user_uid),
                role: "user".to_string(),
                message_type: MessageType::TextMessage,
                content,
                content_token_count: user_token_estimate,
                tl_dr: None,
                tl_dr_token_count: 0,
                status: MessageStatus::Full,
                tool_uses: Vec::new(),
                tool_results: Vec::new(),
                input_tokens: 0,
                timestamp_ms: now_ms(),
            };
            save_message(&user_msg);

            // Update conversation token count
            if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == ContextType::Conversation) {
                ctx.token_count += user_token_estimate;
                ctx.last_refresh_ms = now_ms();
            }

            state.messages.push(user_msg);

            // Create empty assistant message for streaming into
            let assistant_id = format!("A{}", state.next_assistant_id);
            let assistant_uid = format!("UID_{}_A", state.global_next_uid);
            state.next_assistant_id += 1;
            state.global_next_uid += 1;

            let assistant_msg = Message {
                id: assistant_id,
                uid: Some(assistant_uid),
                role: "assistant".to_string(),
                message_type: MessageType::TextMessage,
                content: String::new(),
                content_token_count: 0,
                tl_dr: None,
                tl_dr_token_count: 0,
                status: MessageStatus::Full,
                tool_uses: Vec::new(),
                tool_results: Vec::new(),
                input_tokens: 0,
                timestamp_ms: now_ms(),
            };
            state.messages.push(assistant_msg);

            // Set streaming state
            state.is_streaming = true;
            state.last_stop_reason = None;
            state.streaming_estimated_tokens = 0;
            // Reset per-tick counters (but keep per-stream accumulators since this is auto-continue)
            state.tick_cache_hit_tokens = 0;
            state.tick_cache_miss_tokens = 0;
            state.tick_output_tokens = 0;

            true
        }
        ContinuationAction::Relaunch => {
            // Create empty assistant message for streaming into
            let assistant_id = format!("A{}", state.next_assistant_id);
            let assistant_uid = format!("UID_{}_A", state.global_next_uid);
            state.next_assistant_id += 1;
            state.global_next_uid += 1;

            let assistant_msg = Message {
                id: assistant_id,
                uid: Some(assistant_uid),
                role: "assistant".to_string(),
                message_type: MessageType::TextMessage,
                content: String::new(),
                content_token_count: 0,
                tl_dr: None,
                tl_dr_token_count: 0,
                status: MessageStatus::Full,
                tool_uses: Vec::new(),
                tool_results: Vec::new(),
                input_tokens: 0,
                timestamp_ms: now_ms(),
            };
            state.messages.push(assistant_msg);

            // Set streaming state
            state.is_streaming = true;
            state.last_stop_reason = None;
            state.streaming_estimated_tokens = 0;
            state.tick_cache_hit_tokens = 0;
            state.tick_cache_miss_tokens = 0;
            state.tick_output_tokens = 0;

            true
        }
    }
}
