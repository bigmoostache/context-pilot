//! Reverie trigger system — threshold detection and `optimize_context` tool.
//!
//! Two trigger paths:
//! 1. **Automatic**: context tokens exceed cleaning threshold → fires reverie
//! 2. **Manual**: main AI calls `optimize_context` tool → fires reverie with directive

use crate::state::State;
use cp_base::state::reverie::{Kind, Session};

/// Check whether the context has breached the cleaning threshold and a reverie
/// should be auto-triggered.
///
/// Returns `true` if a reverie was started (caller should begin streaming).
/// Returns `false` if no action was taken (threshold not breached, reverie
/// already active, or reverie disabled).
///
/// Call this after `prepare_stream_context()` has refreshed token counts.
pub(crate) fn check_threshold_trigger(state: &mut State) -> bool {
    // Guard: reverie disabled by user
    if !state.flags.config.reverie_enabled {
        return false;
    }

    // Guard: reverie already running — don't stack 'em
    if state.reveries.contains_key("cleaner") {
        return false;
    }

    // Sum all context element token counts
    let total_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
    let threshold = state.cleaning_threshold_tokens();

    if total_tokens <= threshold {
        return false;
    }

    // Threshold breached — fire the reverie
    // Start the reverie session with the default cleaner agent
    let mut rev = Session::new(Kind::ContextOptimizer, "cleaner".to_string(), None);
    rev.queue_active = true;
    let _r = state.reveries.insert("cleaner".to_string(), rev);

    true
}

/// Start a reverie from the `optimize_context` tool (manual trigger).
///
/// Called by the event loop when it detects the `REVERIE_START:` sentinel
/// in a tool result from `execute_optimize_context()`.
///
/// Returns `true` if the reverie was started, `false` if guards prevented it.
pub(crate) fn start_manual_reverie(state: &mut State, agent_id: String, context: Option<String>) -> bool {
    // Guard: this agent type is already running (one per agent)
    if state.reveries.contains_key(&agent_id) {
        return false;
    }

    // Guard: reverie disabled (the tool handler already checks this,
    // but belt-and-suspenders never hurt a sailor)
    if !state.flags.config.reverie_enabled {
        return false;
    }

    // Start the reverie session
    let mut rev = Session::new(Kind::ContextOptimizer, agent_id.clone(), context);
    rev.queue_active = true;
    let _r = state.reveries.insert(agent_id, rev);

    true
}
