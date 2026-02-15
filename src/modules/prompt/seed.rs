use crate::constants::library;
use crate::state::State;

/// Ensure all built-in agents from library.yaml exist in state, and there's always an active agent.
/// Also loads skills and commands from disk + built-ins.
pub fn ensure_default_agent(state: &mut State) {
    // Load from disk + merge built-ins
    let (mut agents, skills, commands) = super::storage::load_all_prompts();

    // Merge existing state agents that aren't already in the loaded set
    // (handles user-created agents persisted in config.json during migration)
    for existing in &state.agents {
        if !agents.iter().any(|a| a.id == existing.id) {
            agents.push(existing.clone());
        }
    }

    // Sort: default first, then built-in, then user-created
    let default_id = library::default_agent_id();
    agents.sort_by(|a, b| match (a.id == default_id, b.id == default_id) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => match (a.is_builtin, b.is_builtin) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.id.cmp(&b.id),
        },
    });

    state.agents = agents;
    state.skills = skills;
    state.commands = commands;

    // Ensure there's always an active agent
    if let Some(active_id) = &state.active_agent_id {
        // Verify the active agent still exists
        if !state.agents.iter().any(|s| s.id == *active_id) {
            state.active_agent_id = Some(default_id.to_string());
        }
    } else {
        state.active_agent_id = Some(default_id.to_string());
    }
}

/// Get the active agent's content (system prompt)
pub fn get_active_agent_content(state: &State) -> String {
    if let Some(active_id) = &state.active_agent_id
        && let Some(agent) = state.agents.iter().find(|s| &s.id == active_id)
    {
        return agent.content.clone();
    }
    // Fallback to default
    library::agents().first().map(|a| a.content.clone()).unwrap_or_default()
}
