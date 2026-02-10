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
    let mut triggered: Option<&dyn super::continuation::AutoContinuation> = None;
    for &cont in continuations {
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
    for &guard in guard_rails {
        if guard.should_block(state) {
            let reason = guard.block_reason(state);
            // Only create a block notification if one doesn't already exist
            // for this guard rail. Without this check, every main loop tick
            // (8-50ms) would create a new notification when blocked.
            let source_tag = format!("guard_rail:{}", guard.name());
            let already_notified = state.notifications.iter().any(|n| {
                !n.processed
                    && n.notification_type == NotificationType::Custom
                    && n.source == source_tag
            });
            if !already_notified {
                let notif_id = format!("N{}", state.next_notification_id);
                state.next_notification_id += 1;
                state.notifications.push(Notification {
                    id: notif_id,
                    notification_type: NotificationType::Custom,
                    source: source_tag,
                    processed: false,
                    timestamp_ms: now_ms(),
                    content: format!("Auto-continuation blocked by {}: {}", guard.name(), reason),
                });
                state.touch_panel(ContextType::Spine);
            }
            return SpineDecision::Blocked(reason);
        }
    }

    // All guard rails passed — fire the continuation
    let action = cont.build_continuation(state);

    // Update tracking state
    state.spine_config.auto_continuation_count += 1;
    if state.spine_config.autonomous_start_ms.is_none() {
        state.spine_config.autonomous_start_ms = Some(now_ms());
    }

    // No notification created — auto-continuation is under-the-hood TUI behavior,
    // not something the model needs to see in the spine panel.
    state.touch_panel(ContextType::Spine);

    SpineDecision::Continue(action)
}

/// Apply a continuation action to state: create synthetic message, set up for streaming.
///
/// Returns true if a stream should be started (caller should call start_streaming).
pub fn apply_continuation(state: &mut State, action: ContinuationAction) -> bool {
    // Note: UserMessage notifications are marked as processed in
    // prepare_stream_context() — every context rebuild counts as "seen".
    // No need to do it here; it will happen when the stream context is built.

    match action {
        ContinuationAction::SyntheticMessage(content) => {
            state.push_user_message(content);
            state.push_empty_assistant();
            state.begin_streaming();
            true
        }
        ContinuationAction::Relaunch => {
            // Relaunch expects the conversation to already end with a user
            // message.  If it doesn't (defensive), fall back to a tiny
            // synthetic user message so the API always sees alternating roles.
            let last_role = state.messages.iter().rev()
                .find(|m| !m.content.is_empty() || !m.tool_uses.is_empty() || !m.tool_results.is_empty())
                .map(|m| m.role.as_str());

            if last_role != Some("user") {
                state.push_user_message("/* Continue */".to_string());
            }

            state.push_empty_assistant();
            state.begin_streaming();
            true
        }
    }
}
