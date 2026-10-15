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

/// True while inside the exponential backoff window after consecutive
/// continuation errors (2^errors seconds, capped 60s).
fn in_backoff(state: &State) -> bool {
    let cfg = &SpineState::get(state).config;
    let Some(last_err_ms) = cfg.last_continuation_error_ms else {
        return false;
    };
    if cfg.consecutive_continuation_errors == 0 {
        return false;
    }
    let backoff_secs = (1u64 << cfg.consecutive_continuation_errors.min(6)).min(60);
    now_ms().saturating_sub(last_err_ms) < backoff_secs.saturating_mul(1000)
}

/// Whether `content` (trimmed) is a message the spine itself pushed. The
/// auto-continuation prefix is read from the injection template (text before
/// `{count}`), so editing `injections.yaml` cannot silently disable the check.
fn is_synthetic(content: &str) -> bool {
    let spine = &INJECTIONS.spine;
    let prefix = spine.auto_continuation.split("{count}").next().unwrap_or_default().trim();
    (!prefix.is_empty() && content.starts_with(prefix))
        || content == spine.continue_msg.trim()
        || content == spine.reload_complete.trim()
        || content == spine.user_message_during_stream.trim()
}

/// True when the last non-tool-result user message was a synthetic
/// auto-continuation the assistant hasn't answered yet (double-synthetic guard).
fn last_synthetic_unanswered(state: &State) -> bool {
    let synthetic_pos = state
        .agent()
        .messages
        .iter()
        .rposition(|m| m.role == "user" && m.msg_type != cp_base::state::data::message::MsgKind::ToolResult);
    let Some(pos) = synthetic_pos else {
        return false;
    };
    let Some(msg) = state.agent().messages.get(pos) else {
        return false;
    };
    let content = msg.content.trim();
    if !is_synthetic(content) {
        return false;
    }
    // Assistant responded anywhere after the synthetic message (not just last).
    let assistant_responded = state.agent().messages.get(pos..).is_some_and(|slice| {
        slice.iter().any(|m| m.role == "assistant" && (!m.content.is_empty() || !m.tool_uses.is_empty()))
    });
    !assistant_responded
}

/// Run all guard rails; on the first block, emit a deduplicated notification,
/// mark unprocessed notifications as blocked, and return the block reason.
fn check_guard_rails(state: &mut State) -> Option<String> {
    for &guard in all_guard_rails() {
        if !guard.should_block(state) {
            continue;
        }
        let reason = guard.block_reason(state);
        // Notify about the block (create_notification deduplicates internally
        // on kind+source, so no manual check needed).
        let source_tag = format!("guard_rail:{}", guard.name());
        drop(SpineState::create_notification(
            state,
            NotificationType::Custom,
            source_tag,
            format!("Auto-continuation blocked by {}: {}", guard.name(), reason),
        ));
        // Persistent watchers recreate notifications next poll; re-evaluated then.
        SpineState::mark_all_unprocessed_as_blocked(state);
        return Some(reason);
    }
    None
}

/// Evaluate the spine: check for unprocessed notifications, apply guard rails, decide action.
///
/// Returns a `SpineDecision` telling the caller what to do.
/// The caller (app.rs) is responsible for actually starting the stream.
pub fn check_spine(state: &mut State) -> SpineDecision {
    let _fg = cp_base::flame!("check_spine");
    // Never launch if already streaming
    if state.agent().stream.phase.is_streaming() {
        return SpineDecision::Idle;
    }

    // User explicitly stopped streaming (Esc). Absolute hard stop — nothing
    // bypasses it. Notifications queue until the user re-engages (which clears
    // user_stopped via on_user_message).
    if SpineState::get(state).config.user_stopped {
        return SpineDecision::Idle;
    }

    // Backoff after consecutive failed continuations.
    if in_backoff(state) {
        return SpineDecision::Idle;
    }

    // Nothing to do if no unprocessed notifications
    if !SpineState::has_unprocessed_notifications(state) {
        return SpineDecision::Idle;
    }

    // Guardrail 2: no two synthetic messages in a row before the assistant answers.
    if last_synthetic_unanswered(state) {
        return SpineDecision::Idle;
    }

    // Build the continuation action from unprocessed notifications
    let action = build_continuation_from_notifications(state);

    // Check guard rails before firing
    if let Some(reason) = check_guard_rails(state) {
        return SpineDecision::Blocked(reason);
    }

    // All guard rails passed — fire the continuation
    // Mark all consumed notifications as processed so they don't re-trigger
    SpineState::mark_all_unprocessed_as_processed(state);
    let count = &mut SpineState::get_mut(state).config.auto_continuation_count;
    *count = count.saturating_add(1);
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
            .agent()
            .messages
            .iter()
            .rev()
            .find(|m| !m.content.is_empty() || !m.tool_uses.is_empty() || !m.tool_results.is_empty())
            .map(|m| m.role.as_str());

        if last_role == Some("user") {
            ContinuationAction::Relaunch
        } else {
            ContinuationAction::SyntheticMessage(INJECTIONS.spine.user_message_during_stream.trim_end().to_owned())
        }
    } else {
        // Pure ReloadResume
        ContinuationAction::SyntheticMessage(INJECTIONS.spine.reload_complete.trim_end().to_owned())
    }
}

/// Apply a continuation action to state: create synthetic message, set up for streaming.
///
/// Returns true if a stream should be started (caller should call `start_streaming`).
pub fn apply_continuation(state: &mut State, action: ContinuationAction) -> bool {
    // On fire (T736 #3): remove all injected `/* Notification [...] */` messages
    // from the conversation tip. Their content has been consumed into the
    // synthetic continuation message (or is moot on a transparent relaunch), so
    // leaving the raw notification messages behind would duplicate them.
    let _stripped = state.strip_notification_messages();
    match action {
        ContinuationAction::SyntheticMessage(content) => {
            let _idx = state.push_user_message(content);
            let _ai = state.push_empty_assistant();
            state.begin_streaming();
            true
        }
        ContinuationAction::Relaunch => {
            let last_role = state
                .agent()
                .messages
                .iter()
                .rev()
                .find(|m| !m.content.is_empty() || !m.tool_uses.is_empty() || !m.tool_results.is_empty())
                .map(|m| m.role.as_str());

            if last_role != Some("user") {
                let _idx = state.push_user_message(INJECTIONS.spine.continue_msg.trim_end().to_owned());
            }

            let _ai = state.push_empty_assistant();
            state.begin_streaming();
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{NotificationStatus, gc_notifications};

    fn spine_state() -> State {
        let mut state = State::default();
        state.set_ext(SpineState::new());
        state
    }

    fn notify(state: &mut State, source: &str) {
        drop(SpineState::create_notification(state, NotificationType::Custom, source.to_owned(), "x".to_owned()));
    }

    /// Simulate the assistant answering a fired continuation, then the stream ending OK.
    fn answer_and_succeed(state: &mut State) {
        let _ai = state.push_empty_assistant();
        if let Some(last) = state.agent_mut().messages.last_mut() {
            last.content = "done".to_owned();
        }
        SpineState::on_stream_success(state);
    }

    #[test]
    fn guard_caps_a_loop_whose_turns_all_succeed() {
        let mut state = spine_state();
        SpineState::get_mut(&mut state).config.max_auto_retries = Some(2);
        for round in 0u8..2 {
            notify(&mut state, &format!("ping{round}"));
            assert!(matches!(check_spine(&mut state), SpineDecision::Continue(_)));
            answer_and_succeed(&mut state);
        }
        notify(&mut state, "ping2");
        assert!(matches!(check_spine(&mut state), SpineDecision::Blocked(_)));
        assert!(!SpineState::has_unprocessed_notifications(&state));
    }

    #[test]
    fn stream_success_keeps_count_but_clears_backoff() {
        let mut state = spine_state();
        let cfg = &mut SpineState::get_mut(&mut state).config;
        cfg.auto_continuation_count = 3;
        cfg.consecutive_continuation_errors = 2;
        cfg.last_continuation_error_ms = Some(1);
        SpineState::on_stream_success(&mut state);
        let after = SpineState::get(&state).config;
        assert_eq!(after.auto_continuation_count, 3);
        assert_eq!(after.consecutive_continuation_errors, 0);
        assert!(after.last_continuation_error_ms.is_none());
    }

    #[test]
    fn unanswered_synthetic_blocks_a_second_one() {
        let mut state = spine_state();
        notify(&mut state, "a");
        let SpineDecision::Continue(action) = check_spine(&mut state) else { panic!("expected Continue") };
        assert!(apply_continuation(&mut state, action));
        state.agent_mut().stream.phase.transition(cp_base::state::flags::StreamPhase::Idle);
        notify(&mut state, "b");
        assert!(matches!(check_spine(&mut state), SpineDecision::Idle));
    }

    #[test]
    fn gc_drops_processed_then_blocked_never_unprocessed() {
        let mk = |id: &str, status| {
            let mut n = Notification::new(id.to_owned(), NotificationType::Custom, id.to_owned(), String::new());
            n.status = status;
            n
        };
        let mut list = vec![
            mk("N1", NotificationStatus::Blocked),
            mk("N2", NotificationStatus::Unprocessed),
            mk("N3", NotificationStatus::Processed),
            mk("N4", NotificationStatus::Blocked),
        ];
        gc_notifications(&mut list, 2);
        let ids: Vec<&str> = list.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["N2", "N4"]);
    }
}
