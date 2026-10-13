//! Reverie trigger system — threshold detection and `optimize_context` tool.
//!
//! Two trigger paths:
//! 1. **Automatic**: context tokens exceed cleaning threshold → fires reverie
//! 2. **Manual**: main AI calls `optimize_context` tool → fires reverie with directive

use crate::state::State;
use cp_base::state::data::model_helpers::ModelPricing as _;
use cp_base::state::runtime::reverie::{Kind, Session};

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

    // Guard: a cleaner reverie is already running FOR THIS AGENT. The limit is
    // one reverie per (agent, agent), not one globally — a background agent
    // cleaning its own context must not block another agent from cleaning its.
    let owner = owner_agent_id(state);
    let slot = reverie_slot(owner.as_deref(), "cleaner");
    if state.reveries.contains_key(&slot) {
        return false;
    }

    // Sum all context element token counts
    let total_tokens: usize = state.agent().context.iter().map(|c| c.token_count).sum();
    let threshold = state.cleaning_threshold_tokens();

    if total_tokens <= threshold {
        return false;
    }

    // Threshold breached — fire the reverie
    // Start the reverie session with the default cleaner agent
    let mut rev = Session::new(Kind::ContextOptimizer, "cleaner".to_owned(), None);
    rev.queue_active = true;
    rev.agent_id = owner;
    let _r = state.reveries.insert(slot, rev);

    true
}

/// Build the map slot key for a reverie: `"{agent_id}\u{1}{agent_id}"`, or just
/// `system_id` when no owner thread is known (cold boot / single-thread).
///
/// `state.reveries` and `App::reverie_streams` are both keyed by this slot so
/// the running-reverie limit is scoped to a single (agent, agent) pair rather
/// than globally per agent type. The agent's *identity* (for prompt loading) is
/// never derived from this key — it lives on [`Session::system_id`].
#[must_use]
pub(crate) fn reverie_slot(agent_id: Option<&str>, system_id: &str) -> String {
    agent_id.map_or_else(|| system_id.to_owned(), |tid| format!("{tid}\u{1}{system_id}"))
}

/// Resolve the agent that owns the current context (the executing agent).
/// Reverie lifecycle notifications are routed back here so they land on the
/// launching agent, not wherever focus drifts while the reverie runs.
pub(crate) fn owner_agent_id(state: &State) -> Option<String> {
    state.executing_agent_id().map(str::to_owned)
}

/// Start a reverie from the `optimize_context` tool (manual trigger).
///
/// Called by the event loop when it detects the `REVERIE_START:` sentinel
/// in a tool result from `execute_optimize_context()`.
///
/// Returns `true` if the reverie was started, `false` if guards prevented it.
pub(crate) fn start_manual_reverie(state: &mut State, system_id: String, context: Option<String>) -> bool {
    // Guard: this agent type is already running FOR THIS AGENT (one reverie per
    // (agent, agent), not one globally — see `reverie_slot`).
    let owner = owner_agent_id(state);
    let slot = reverie_slot(owner.as_deref(), &system_id);
    if state.reveries.contains_key(&slot) {
        return false;
    }

    // Guard: reverie disabled (the tool handler already checks this,
    // but belt-and-suspenders never hurt a sailor)
    if !state.flags.config.reverie_enabled {
        return false;
    }

    // Start the reverie session
    let mut rev = Session::new(Kind::ContextOptimizer, system_id, context);
    rev.queue_active = true;
    rev.agent_id = owner;
    let _r = state.reveries.insert(slot, rev);

    true
}
