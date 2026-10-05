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
///
/// Only [`MaxAutoRetriesGuard`] remains. The output-token, duration, and
/// message-count guards were REMOVED (Phase H): in a thread-centric fleet that
/// advances indefinitely, a *global* ceiling on cumulative output / wall-clock /
/// message count is a single-worker fossil — it would halt the whole agent on a
/// budget that no longer maps to any one unit of work. `MaxAutoRetries` (the
/// anti-runaway cap on consecutive auto-continuations without human input) is
/// the only limit that still makes sense per the §13 ruling. (Same removal
/// pattern as the earlier `MaxCost` drop.)
pub(crate) fn all_guard_rails() -> &'static [&'static dyn GuardRailStopLogic] {
    static GUARD_RAILS: &[&dyn GuardRailStopLogic] = &[&MaxAutoRetriesGuard];
    GUARD_RAILS
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
