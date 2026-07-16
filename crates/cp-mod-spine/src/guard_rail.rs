use cp_base::panels::now_ms;
use cp_base::state::runtime::State;

use crate::types::SpineState;

/// Trait for guard rail safety limits.
///
/// Guard rails are checked BEFORE any auto-continuation fires.
/// If any guard rail returns `should_block() == true`, no auto-continuation
/// will happen — the system will stop and wait for human input.
///
/// All guard rails are parameterized via `SpineConfig` and are nullable
/// (disabled by default).
pub(crate) trait GuardRailStopLogic: Send + Sync {
    /// Human-readable name for logging/debugging
    fn name(&self) -> &'static str;

    /// Check if this guard rail should block auto-continuation.
    /// Returns true if the limit has been exceeded.
    fn should_block(&self, state: &State) -> bool;

    /// Human-readable reason for why continuation was blocked.
    /// Only called if `should_block()` returned true.
    fn block_reason(&self, state: &State) -> String;
}

/// Collect all registered guard rail implementations.
///
/// All guard rails are checked — if ANY blocks, continuation is prevented.
pub(crate) fn all_guard_rails() -> &'static [&'static dyn GuardRailStopLogic] {
    static GUARD_RAILS: &[&dyn GuardRailStopLogic] =
        &[&MaxOutputTokensGuard, &MaxDurationGuard, &MaxMessagesGuard, &MaxAutoRetriesGuard];
    GUARD_RAILS
}

// ============================================================================
// Implementation: MaxOutputTokensGuard
// ============================================================================

/// Block if total output tokens exceed the configured limit.
pub(crate) struct MaxOutputTokensGuard;

impl GuardRailStopLogic for MaxOutputTokensGuard {
    fn name(&self) -> &'static str {
        "MaxOutputTokens"
    }

    fn should_block(&self, state: &State) -> bool {
        SpineState::get(state).config.max_output_tokens.is_some_and(|max| state.total_output_tokens >= max)
    }

    fn block_reason(&self, state: &State) -> String {
        format!(
            "Output token limit reached: {} / {} tokens",
            state.total_output_tokens,
            SpineState::get(state).config.max_output_tokens.unwrap_or(0)
        )
    }
}

// ============================================================================
// Implementation: MaxDurationGuard
// ============================================================================

/// Block if autonomous operation has exceeded the configured time limit.
/// Tracks time from `autonomous_start_ms` (set when first auto-continuation fires).
pub(crate) struct MaxDurationGuard;

impl GuardRailStopLogic for MaxDurationGuard {
    fn name(&self) -> &'static str {
        "MaxDuration"
    }

    fn should_block(&self, state: &State) -> bool {
        if let (Some(max_secs), Some(start_ms)) =
            (SpineState::get(state).config.max_duration_secs, SpineState::get(state).config.autonomous_start_ms)
        {
            let elapsed_ms = now_ms().saturating_sub(start_ms);
            let elapsed_secs = cp_base::panels::time_arith::ms_to_secs(elapsed_ms);
            elapsed_secs >= max_secs
        } else {
            false
        }
    }

    fn block_reason(&self, state: &State) -> String {
        let elapsed_secs = SpineState::get(state)
            .config
            .autonomous_start_ms
            .map_or(0, |start| cp_base::panels::time_arith::ms_to_secs(now_ms().saturating_sub(start)));
        format!(
            "Duration limit reached: {}s / {}s",
            elapsed_secs,
            SpineState::get(state).config.max_duration_secs.unwrap_or(0)
        )
    }
}

// ============================================================================
// Implementation: MaxMessagesGuard
// ============================================================================

/// Block if conversation message count exceeds the configured limit.
pub(crate) struct MaxMessagesGuard;

impl GuardRailStopLogic for MaxMessagesGuard {
    fn name(&self) -> &'static str {
        "MaxMessages"
    }

    fn should_block(&self, state: &State) -> bool {
        SpineState::get(state).config.max_messages.is_some_and(|max| state.messages.len() >= max)
    }

    fn block_reason(&self, state: &State) -> String {
        format!(
            "Message limit reached: {} / {} messages",
            state.messages.len(),
            SpineState::get(state).config.max_messages.unwrap_or(0)
        )
    }
}

// ============================================================================
// Implementation: MaxAutoRetriesGuard
// ============================================================================

/// Block if auto-continuation count exceeds the configured limit.
/// Tracks consecutive auto-continuations without human input.
/// The counter is reset when the user sends a message.
pub(crate) struct MaxAutoRetriesGuard;

impl GuardRailStopLogic for MaxAutoRetriesGuard {
    fn name(&self) -> &'static str {
        "MaxAutoRetries"
    }

    fn should_block(&self, state: &State) -> bool {
        SpineState::get(state)
            .config
            .max_auto_retries
            .is_some_and(|max| SpineState::get(state).config.auto_continuation_count >= max)
    }

    fn block_reason(&self, state: &State) -> String {
        format!(
            "Auto-retry limit reached: {} / {} continuations",
            SpineState::get(state).config.auto_continuation_count,
            SpineState::get(state).config.max_auto_retries.unwrap_or(0)
        )
    }
}
