use crate::constants::prompts;
use crate::state::{State, SystemItem};

/// Ensure all built-in seeds from prompts.yaml exist in state, and there's always an active seed.
pub fn ensure_default_seed(state: &mut State) {
    // Ensure all seeds from prompts.yaml exist
    for seed in prompts::seeds() {
        let exists = state.systems.iter().any(|s| s.id == seed.id);
        if !exists {
            state.systems.push(SystemItem {
                id: seed.id.clone(),
                name: seed.name.clone(),
                description: seed.description.clone(),
                content: seed.content.clone(),
            });
        }
    }

    // Sort so S0 (default) comes first, then S_ prefixed (built-in), then user-created
    state.systems.sort_by(|a, b| {
        let a_builtin = a.id.starts_with("S_") || a.id == prompts::default_seed_id();
        let b_builtin = b.id.starts_with("S_") || b.id == prompts::default_seed_id();
        match (a.id == prompts::default_seed_id(), b.id == prompts::default_seed_id()) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => match (a_builtin, b_builtin) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.id.cmp(&b.id),
            }
        }
    });

    // Ensure there's always an active seed
    if state.active_system_id.is_none() {
        state.active_system_id = Some(prompts::default_seed_id().to_string());
    } else {
        // Verify the active seed still exists
        let active_id = state.active_system_id.as_ref().unwrap();
        if !state.systems.iter().any(|s| &s.id == active_id) {
            state.active_system_id = Some(prompts::default_seed_id().to_string());
        }
    }

    // Update next_system_id if needed
    let max_id: usize = state.systems.iter()
        .filter_map(|s| s.id.strip_prefix('S').and_then(|n| n.parse().ok()))
        .max()
        .unwrap_or(0);
    if state.next_system_id <= max_id {
        state.next_system_id = max_id + 1;
    }
}

/// Get the active seed's content (system prompt)
pub fn get_active_seed_content(state: &State) -> String {
    if let Some(active_id) = &state.active_system_id {
        if let Some(system) = state.systems.iter().find(|s| &s.id == active_id) {
            return system.content.clone();
        }
    }
    // Fallback to default (shouldn't happen if ensure_default_seed was called)
    prompts::default_seed_content().to_string()
}
