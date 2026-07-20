use cp_base::config::accessors::library;
use cp_base::state::runtime::State;

use crate::types::{PromptState, PromptType};

/// Ensure there's always a valid active agent ID.
/// Dynamically loads agents from disk + built-ins to verify.
pub fn ensure_default_agent(state: &mut State) {
    let agents = crate::storage::load_prompts_for(PromptType::Agent);
    let default_id = library::default_agent_id();

    let ps_mut = PromptState::get_mut(state);
    if let Some(active_id) = ps_mut.active_agent_id.as_ref() {
        if !agents.iter().any(|a| a.id == *active_id) {
            ps_mut.active_agent_id = Some(default_id.to_owned());
        }
    } else {
        ps_mut.active_agent_id = Some(default_id.to_owned());
    }
}

/// Get the active agent's content (system prompt).
/// Dynamically loads from disk every call — no caching.
#[must_use]
pub fn get_active_agent_content(state: &State) -> String {
    let ps = PromptState::get(state);
    let agents = crate::storage::load_prompts_for(PromptType::Agent);
    if let Some(active_id) = ps.active_agent_id.as_ref()
        && let Some(agent) = agents.iter().find(|a| &a.id == active_id)
    {
        return agent.content.clone();
    }
    // Fallback to default
    library::agents().first().map(|a| a.content.clone()).unwrap_or_default()
}
