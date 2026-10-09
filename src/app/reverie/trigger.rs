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

    // Guard: a cleaner reverie is already running FOR THIS THREAD. The limit is
    // one reverie per (thread, agent), not one globally — a background thread
    // cleaning its own context must not block another thread from cleaning its.
    let owner = owner_thread_id(state);
    let slot = reverie_slot(owner.as_deref(), "cleaner");
    if state.reveries.contains_key(&slot) {
        return false;
    }

    // Sum all context element token counts
    let total_tokens: usize = state.thread().context.iter().map(|c| c.token_count).sum();
    let threshold = state.cleaning_threshold_tokens();

    if total_tokens <= threshold {
        return false;
    }

    // Threshold breached — fire the reverie
    // Start the reverie session with the default cleaner agent
    let mut rev = Session::new(Kind::ContextOptimizer, "cleaner".to_owned(), None);
    rev.queue_active = true;
    rev.thread_id = owner;
    let _r = state.reveries.insert(slot, rev);

    true
}

/// Build the map slot key for a reverie: `"{thread_id}\u{1}{agent_id}"`, or just
/// `agent_id` when no owner thread is known (cold boot / single-thread).
///
/// `state.reveries` and `App::reverie_streams` are both keyed by this slot so
/// the running-reverie limit is scoped to a single (thread, agent) pair rather
/// than globally per agent type. The agent's *identity* (for prompt loading) is
/// never derived from this key — it lives on [`Session::agent_id`].
#[must_use]
pub(crate) fn reverie_slot(thread_id: Option<&str>, agent_id: &str) -> String {
    thread_id.map_or_else(|| agent_id.to_owned(), |tid| format!("{tid}\u{1}{agent_id}"))
}

/// Resolve the thread that owns the current context (the executing thread).
/// Reverie lifecycle notifications are routed back here so they land on the
/// launching thread, not wherever focus drifts while the reverie runs.
pub(crate) fn owner_thread_id(state: &State) -> Option<String> {
    state.executing_thread_id().map(str::to_owned)
}

/// Start a reverie from the `optimize_context` tool (manual trigger).
///
/// Called by the event loop when it detects the `REVERIE_START:` sentinel
/// in a tool result from `execute_optimize_context()`.
///
/// Returns `true` if the reverie was started, `false` if guards prevented it.
pub(crate) fn start_manual_reverie(state: &mut State, agent_id: String, context: Option<String>) -> bool {
    // Guard: this agent type is already running FOR THIS THREAD (one reverie per
    // (thread, agent), not one globally — see `reverie_slot`).
    let owner = owner_thread_id(state);
    let slot = reverie_slot(owner.as_deref(), &agent_id);
    if state.reveries.contains_key(&slot) {
        return false;
    }

    // Guard: reverie disabled (the tool handler already checks this,
    // but belt-and-suspenders never hurt a sailor)
    if !state.flags.config.reverie_enabled {
        return false;
    }

    // Start the reverie session
    let mut rev = Session::new(Kind::ContextOptimizer, agent_id, context);
    rev.queue_active = true;
    rev.thread_id = owner;
    let _r = state.reveries.insert(slot, rev);

    true
}
