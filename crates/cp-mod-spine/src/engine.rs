//! Spine engine — the central check that evaluates auto-continuation and guard rails.
//!
//! Called from app.rs both periodically (main loop) and synchronously (after InputSubmit).
//! Auto-continuation is driven entirely by notifications:
//! - UserMessage / ReloadResume → synthetic message or relaunch
//! - Custom (from watchers, coucou, context threshold) → synthetic message
//!
//! No more AutoContinuation trait — all triggers go through the watcher → notification pipeline.

use cp_base::config::INJECTIONS;
use cp_base::panels::now_ms;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;

use crate::guard_rail::all_guard_rails;
use crate::types::{ContinuationAction, Notification, NotificationType, SpineState};

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

/// Evaluate the spine: check for unprocessed notifications, apply guard rails, decide action.
///
/// Returns a `SpineDecision` telling the caller what to do.
/// The caller (app.rs) is responsible for actually starting the stream.
pub fn check_spine(state: &mut State) -> SpineDecision {
    // Never launch if already streaming
    if state.flags.stream.phase.is_streaming() {
        return SpineDecision::Idle;
    }

    // Backoff after consecutive failed continuations (errors with all retries exhausted).
    // Delay: 2^errors seconds, capped at 60s. Prevents runaway loops on persistent API failures.
    {
        let cfg = &SpineState::get(state).config;
        if cfg.consecutive_continuation_errors > 0
            && let Some(last_err_ms) = cfg.last_continuation_error_ms
        {
            let backoff_secs = (1u64 << cfg.consecutive_continuation_errors.min(6)).min(60);
            let elapsed_ms = now_ms().saturating_sub(last_err_ms);
            if elapsed_ms < backoff_secs.saturating_mul(1000) {
                return SpineDecision::Idle;
            }
        }
    }

    // Nothing to do if no unprocessed notifications
    if !SpineState::has_unprocessed_notifications(state) {
        return SpineDecision::Idle;
    }

    // If any unprocessed notification comes from an external human channel
    // (chat/bridge), treat it like a fresh user message: reset all counters
    // so guard rails don't block what is effectively human-initiated input.
    {
        let has_external =
            SpineState::get(state).notifications.iter().any(|n| n.is_unprocessed() && n.source == "chat");
        if has_external {
            let cfg = &mut SpineState::get_mut(state).config;
            cfg.auto_continuation_count = 0;
            cfg.autonomous_start_ms = None;
            cfg.consecutive_continuation_errors = 0;
            cfg.last_continuation_error_ms = None;
        }
    }

    // Check if user explicitly stopped (Esc) — only block todo-driven continuations,
    // not new triggers like chat messages or coucou timers. A fresh notification
    // (Custom/UserMessage) always gets through — the user intended to wake us.
    if SpineState::get(state).config.user_stopped {
        let all_todo = SpineState::get(state)
            .notifications
            .iter()
            .filter(|n| n.is_unprocessed())
            .all(|n| n.source == "todo_continuation");
        if all_todo {
            return SpineDecision::Idle;
        }
        // Non-todo notification present — clear user_stopped and proceed
        SpineState::get_mut(state).config.user_stopped = false;
    }

    // === Guardrail 2: No two synthetic messages in a row ===
    // If the last non-error user message was a synthetic auto-continuation AND
    // the assistant hasn't responded yet, don't fire another one.
    // Once the assistant has responded (stream completed), it's safe to inject
    // a new synthetic message for the next notification.
    {
        let last_non_error_user = state
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user" && m.msg_type != cp_base::state::data::message::MsgKind::ToolResult);
        if let Some(msg) = last_non_error_user {
            let content = msg.content.trim();
            let is_synthetic = content.starts_with("/* Auto-continuation:")
                || content == INJECTIONS.spine.continue_msg.trim()
                || content == INJECTIONS.spine.reload_complete.trim();
            if is_synthetic {
                // Check if the assistant has responded after this synthetic message.
                // If the last message (any role) is still this user message or another
                // user message, the LLM hasn't processed it yet — block.
                let last_msg = state.messages.last();
                let assistant_responded = last_msg
                    .is_some_and(|m| m.role == "assistant" && (!m.content.is_empty() || !m.tool_uses.is_empty()));
                if !assistant_responded {
                    return SpineDecision::Idle;
                }
            }
        }
    }

    // Build the continuation action from unprocessed notifications
    let action = build_continuation_from_notifications(state);

    // Check guard rails before firing
    let guard_rails = all_guard_rails();
    for &guard in guard_rails {
        if guard.should_block(state) {
            let reason = guard.block_reason(state);
            // Deduplicate block notifications
            let source_tag = format!("guard_rail:{}", guard.name());
            let already_notified = SpineState::get(state)
                .notifications
                .iter()
                .any(|n| !n.is_processed() && n.kind == NotificationType::Custom && n.source == source_tag);
            if !already_notified {
                drop(SpineState::create_notification(
                    state,
                    NotificationType::Custom,
                    source_tag,
                    format!("Auto-continuation blocked by {}: {}", guard.name(), reason),
                ));
            }
            // Mark all unprocessed notifications as blocked — they were evaluated
            // and the decision was "blocked." Persistent watchers will recreate new
            // notifications on the next poll, and we'll re-evaluate then.
            SpineState::mark_all_unprocessed_as_blocked(state);

            return SpineDecision::Blocked(reason);
        }
    }

    // All guard rails passed — fire the continuation
    let count = &mut SpineState::get_mut(state).config.auto_continuation_count;
    *count = count.saturating_add(1);
    if SpineState::get(state).config.autonomous_start_ms.is_none() {
        SpineState::get_mut(state).config.autonomous_start_ms = Some(now_ms());
    }
    state.touch_panel(Kind::SPINE);

    SpineDecision::Continue(action)
}

/// Build a `ContinuationAction` directly from unprocessed notifications.
///
/// Logic:
/// - If ALL unprocessed are transparent (`UserMessage` / `ReloadResume`), handle simply
/// - Otherwise, build a synthetic message explaining the notifications
fn build_continuation_from_notifications(state: &State) -> ContinuationAction {
    let unprocessed = SpineState::unprocessed_notifications(state);

    let all_transparent =
        unprocessed.iter().all(|n| matches!(n.kind, NotificationType::UserMessage | NotificationType::ReloadResume));

    if all_transparent {
        return build_transparent_continuation(&unprocessed, state);
    }

    // Non-transparent notifications — build explanatory synthetic message
    let explain: Vec<&Notification> = unprocessed
        .iter()
        .filter(|n| !matches!(n.kind, NotificationType::UserMessage | NotificationType::ReloadResume))
        .copied()
        .collect();

    let mut parts = Vec::new();
    for n in &explain {
        parts.push(format!("[{}] {} — {}", n.id, n.kind.label(), n.content));
    }
    let msg = INJECTIONS
        .spine
        .auto_continuation
        .trim_end()
        .replace("{count}", &explain.len().to_string())
        .replace("{details}", &parts.join("\n"));
    ContinuationAction::SyntheticMessage(msg)
}

/// Handle transparent notifications (`UserMessage` / `ReloadResume`).
fn build_transparent_continuation(unprocessed: &[&Notification], state: &State) -> ContinuationAction {
    let has_user_message = unprocessed.iter().any(|n| n.kind == NotificationType::UserMessage);

    if has_user_message {
        // User sent a message — check if conversation already ends with user turn
        let last_role = state
            .messages
            .iter()
            .rev()
            .find(|m| !m.content.is_empty() || !m.tool_uses.is_empty() || !m.tool_results.is_empty())
            .map(|m| m.role.as_str());

        if last_role == Some("user") {
            ContinuationAction::Relaunch
        } else {
            ContinuationAction::SyntheticMessage(INJECTIONS.spine.user_message_during_stream.trim_end().to_string())
        }
    } else {
        // Pure ReloadResume
        ContinuationAction::SyntheticMessage(INJECTIONS.spine.reload_complete.trim_end().to_string())
    }
}

/// Apply a continuation action to state: create synthetic message, set up for streaming.
///
/// Returns true if a stream should be started (caller should call `start_streaming`).
pub fn apply_continuation(state: &mut State, action: ContinuationAction) -> bool {
    match action {
        ContinuationAction::SyntheticMessage(content) => {
            let _ = state.push_user_message(content);
            let _ = state.push_empty_assistant();
            state.begin_streaming();
            true
        }
        ContinuationAction::Relaunch => {
            let last_role = state
                .messages
                .iter()
                .rev()
                .find(|m| !m.content.is_empty() || !m.tool_uses.is_empty() || !m.tool_results.is_empty())
                .map(|m| m.role.as_str());

            if last_role != Some("user") {
                let _ = state.push_user_message(INJECTIONS.spine.continue_msg.trim_end().to_string());
            }

            let _ = state.push_empty_assistant();
            state.begin_streaming();
            true
        }
    }
}
