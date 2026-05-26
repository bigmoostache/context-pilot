//! History cleanup trap — blocks queue execution until old conversation
//! history panels are closed, preventing unbounded accumulation.
//!
//! # Trigger
//! When `Queue_execute` fires and ≥ [`TRAP_THRESHOLD`] conversation history
//! panels are open, the queue flush is suppressed and the trap activates.
//!
//! # While active
//! Every tool call except `Close_conversation_history` is rejected by the
//! pre-flight check in [`trap_blocks_tool`].  `Close_conversation_history`
//! also bypasses the queue intercept so it executes immediately.
//!
//! # Deactivation
//! After each `Close_conversation_history` execution, [`maybe_deactivate_trap`]
//! checks whether fewer than [`TRAP_THRESHOLD`] history panels remain.
//! If so the trap deactivates and normal tool execution resumes.

use cp_base::config::INJECTIONS;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_mod_queue::types::QueueState;

/// Minimum number of conversation history panels that triggers the trap.
const TRAP_THRESHOLD: usize = 3;

/// Number of most-recent panels the AI may optionally keep.
const OPTIONAL_KEEP: usize = 2;

// ─── Public API ─────────────────────────────────────────────────────────────

/// Check trigger conditions and activate the trap if met.
///
/// Called from `pipeline.rs` when `Queue_execute` fires.
/// Returns `Some(message)` if the trap activates (queue must NOT flush),
/// or `None` if conditions are not met (queue proceeds normally).
pub(crate) fn check_and_trigger_trap(state: &mut State) -> Option<String> {
    // Already active — re-emit the blocked message instead of re-triggering
    if QueueState::get(state).trap_active {
        return Some(format_blocked_message(state));
    }

    // Collect history panels sorted oldest → newest
    let mut panels: Vec<(String, u64)> = state
        .context
        .iter()
        .filter(|c| c.context_type.as_str() == Kind::CONVERSATION_HISTORY)
        .map(|c| (c.id.clone(), c.last_refresh_ms))
        .collect();

    if panels.len() < TRAP_THRESHOLD {
        return None;
    }

    panels.sort_by_key(|(_, ms)| *ms);

    let all_ids: Vec<String> = panels.iter().map(|(id, _)| id.clone()).collect();
    let optional_ids: Vec<String> = all_ids.iter().rev().take(OPTIONAL_KEEP).cloned().collect();

    // Activate trap
    let qs = QueueState::get_mut(state);
    qs.trap_active = true;
    qs.trap_panel_ids.clone_from(&all_ids);
    qs.trap_optional_ids.clone_from(&optional_ids);

    // Format trigger message from YAML template
    let panels_str = all_ids.join(", ");
    let opt_1 = optional_ids.first().map_or("?", String::as_str);
    let opt_2 = optional_ids.get(1).map_or("?", String::as_str);

    let msg = INJECTIONS
        .trap
        .history_cleanup_triggered
        .trim_end()
        .replace("%PANELS%", &panels_str)
        .replace("%OPTIONAL_1%", opt_1)
        .replace("%OPTIONAL_2%", opt_2);

    Some(msg)
}

/// If the trap is active, check whether `tool_name` is blocked.
///
/// Called from `pre_flight_tool()` before every tool execution.
/// Returns `Some(error_message)` if the tool is blocked, `None` if allowed.
pub(crate) fn trap_blocks_tool(tool_name: &str, state: &State) -> Option<String> {
    let qs = QueueState::get(state);
    if !qs.trap_active {
        return None;
    }
    if tool_name == "Close_conversation_history" || tool_name == "Think" {
        return None;
    }
    Some(format_blocked_message(state))
}

/// Deactivate the trap if fewer than [`TRAP_THRESHOLD`] history panels remain.
///
/// Called from `pipeline.rs` after `Close_conversation_history` executes.
pub(crate) fn maybe_deactivate_trap(state: &mut State) {
    if !QueueState::get(state).trap_active {
        return;
    }

    let remaining = state.context.iter().filter(|c| c.context_type.as_str() == Kind::CONVERSATION_HISTORY).count();

    if remaining < TRAP_THRESHOLD {
        let qs = QueueState::get_mut(state);
        qs.trap_active = false;
        qs.trap_panel_ids.clear();
        qs.trap_optional_ids.clear();
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Build the "blocked" message listing panels still open.
fn format_blocked_message(state: &State) -> String {
    let remaining: Vec<String> = state
        .context
        .iter()
        .filter(|c| c.context_type.as_str() == Kind::CONVERSATION_HISTORY)
        .map(|c| c.id.clone())
        .collect();

    INJECTIONS.trap.history_cleanup_blocked.trim_end().replace("%REMAINING_PANELS%", &remaining.join(", "))
}
