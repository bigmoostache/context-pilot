use crate::state::State;

/// Trait for guard rail safety limits.
///
/// Guard rails are checked BEFORE any auto-continuation fires.
/// If any guard rail returns `should_block() == true`, no auto-continuation
/// will happen — the system will stop and wait for human input.
///
/// All guard rails are parameterized via `SpineConfig` and are nullable
/// (disabled by default).
pub trait GuardRailStopLogic: Send + Sync {
    /// Human-readable name for logging/debugging
    fn name(&self) -> &str;

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
pub fn all_guard_rails() -> &'static [&'static dyn GuardRailStopLogic] {
    static GUARD_RAILS: &[&dyn GuardRailStopLogic] = &[
        &MaxOutputTokensGuard,
        &MaxCostGuard,
        &MaxDurationGuard,
        &MaxMessagesGuard,
        &MaxAutoRetriesGuard,
    ];
    GUARD_RAILS
}

// ============================================================================
// Implementation: MaxOutputTokensGuard
// ============================================================================

/// Block if total output tokens exceed the configured limit.
pub struct MaxOutputTokensGuard;

impl GuardRailStopLogic for MaxOutputTokensGuard {
    fn name(&self) -> &str { "MaxOutputTokens" }

    fn should_block(&self, state: &State) -> bool {
        if let Some(max) = state.spine_config.max_output_tokens {
            state.total_output_tokens >= max
        } else {
            false
        }
    }

    fn block_reason(&self, state: &State) -> String {
        format!(
            "Output token limit reached: {} / {} tokens",
            state.total_output_tokens,
            state.spine_config.max_output_tokens.unwrap_or(0)
        )
    }
}

// ============================================================================
// Implementation: MaxCostGuard
// ============================================================================

/// Block if estimated session cost exceeds the configured USD limit.
pub struct MaxCostGuard;

impl GuardRailStopLogic for MaxCostGuard {
    fn name(&self) -> &str { "MaxCost" }

    fn should_block(&self, state: &State) -> bool {
        if let Some(max_cost) = state.spine_config.max_cost {
            let current_cost = Self::calculate_cost(state);
            current_cost >= max_cost
        } else {
            false
        }
    }

    fn block_reason(&self, state: &State) -> String {
        let current_cost = Self::calculate_cost(state);
        format!(
            "Cost limit reached: ${:.4} / ${:.4}",
            current_cost,
            state.spine_config.max_cost.unwrap_or(0.0)
        )
    }
}

impl MaxCostGuard {
    fn calculate_cost(state: &State) -> f64 {
        let hit_cost = State::token_cost(state.cache_hit_tokens, state.cache_hit_price_per_mtok());
        let miss_cost = State::token_cost(state.cache_miss_tokens, state.cache_miss_price_per_mtok());
        let output_cost = State::token_cost(state.total_output_tokens, state.output_price_per_mtok());
        hit_cost + miss_cost + output_cost
    }
}

// ============================================================================
// Implementation: MaxDurationGuard
// ============================================================================

/// Block if autonomous operation has exceeded the configured time limit.
/// Tracks time from `autonomous_start_ms` (set when first auto-continuation fires).
pub struct MaxDurationGuard;

impl GuardRailStopLogic for MaxDurationGuard {
    fn name(&self) -> &str { "MaxDuration" }

    fn should_block(&self, state: &State) -> bool {
        if let (Some(max_secs), Some(start_ms)) = (
            state.spine_config.max_duration_secs,
            state.spine_config.autonomous_start_ms,
        ) {
            let elapsed_ms = crate::core::panels::now_ms().saturating_sub(start_ms);
            let elapsed_secs = elapsed_ms / 1000;
            elapsed_secs >= max_secs
        } else {
            false
        }
    }

    fn block_reason(&self, state: &State) -> String {
        let elapsed_secs = state.spine_config.autonomous_start_ms
            .map(|start| crate::core::panels::now_ms().saturating_sub(start) / 1000)
            .unwrap_or(0);
        format!(
            "Duration limit reached: {}s / {}s",
            elapsed_secs,
            state.spine_config.max_duration_secs.unwrap_or(0)
        )
    }
}

// ============================================================================
// Implementation: MaxMessagesGuard
// ============================================================================

/// Block if conversation message count exceeds the configured limit.
pub struct MaxMessagesGuard;

impl GuardRailStopLogic for MaxMessagesGuard {
    fn name(&self) -> &str { "MaxMessages" }

    fn should_block(&self, state: &State) -> bool {
        if let Some(max) = state.spine_config.max_messages {
            state.messages.len() >= max
        } else {
            false
        }
    }

    fn block_reason(&self, state: &State) -> String {
        format!(
            "Message limit reached: {} / {} messages",
            state.messages.len(),
            state.spine_config.max_messages.unwrap_or(0)
        )
    }
}

// ============================================================================
// Implementation: MaxAutoRetriesGuard
// ============================================================================

/// Block if auto-continuation count exceeds the configured limit.
/// Tracks consecutive auto-continuations without human input.
/// The counter is reset when the user sends a message.
pub struct MaxAutoRetriesGuard;

impl GuardRailStopLogic for MaxAutoRetriesGuard {
    fn name(&self) -> &str { "MaxAutoRetries" }

    fn should_block(&self, state: &State) -> bool {
        if let Some(max) = state.spine_config.max_auto_retries {
            state.spine_config.auto_continuation_count >= max
        } else {
            false
        }
    }

    fn block_reason(&self, state: &State) -> String {
        format!(
            "Auto-retry limit reached: {} / {} continuations",
            state.spine_config.auto_continuation_count,
            state.spine_config.max_auto_retries.unwrap_or(0)
        )
    }
}
