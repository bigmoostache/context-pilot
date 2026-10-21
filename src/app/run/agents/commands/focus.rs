//! `FocusAgent` — move the human's focus to an agent from the web sidebar.
//!
//! Same effect as the TUI's arrow-key selection: only
//! [`FocusState::focused_agent_id`] is set; the loop's `follow_focus` makes the
//! agent executing next tick and the focus observer journals the
//! `AgentFocusChanged` delta, so the web footer follows.

use cp_base::state::runtime::State;
use cp_mod_agents::types::{AgentsState, FocusState};

/// Focus `agent_id`; an unknown id is logged and ignored.
pub(super) fn apply_focus_agent(state: &mut State, agent_id: &str) {
    if !AgentsState::get(state).agents.iter().any(|a| a.id == agent_id) {
        log::warn!("bridge: FocusAgent for unknown agent {agent_id}");
        return;
    }
    FocusState::get_mut(state).focused_agent_id = Some(agent_id.to_owned());
    state.flags.ui.dirty = true;
}
