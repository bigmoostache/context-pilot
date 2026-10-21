//! `RenameAgent` — set an agent's display name from the web command.
//!
//! Only mutates `agent.name`; the rename chokepoint
//! ([`emit_agent_renamed`](super::super::paused::emit_agent_renamed)) journals
//! the `AgentRenamed` delta on the next tick.

use cp_base::state::runtime::State;
use cp_mod_agents::types::AgentsState;

/// Rename `agent_id` to the trimmed `name`; a blank name or unknown id is logged and ignored.
pub(super) fn apply_rename_agent(state: &mut State, agent_id: &str, name: &str) {
    match AgentsState::get_mut(state).rename(agent_id, name) {
        Ok(()) => state.flags.ui.dirty = true,
        Err(e) => log::warn!("bridge: RenameAgent {agent_id}: {e}"),
    }
}
