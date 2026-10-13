use cp_base::config::accessors::library;
use cp_base::state::runtime::State;

use crate::types::{PromptState, PromptType};

/// Ensure there's always a valid active system ID.
/// Dynamically loads systems from disk + built-ins to verify.
pub fn ensure_default_system(state: &mut State) {
    // Fold any per-realm behaviour files into the fleet-shared home dir before
    // the first load, so migrated systems are visible to the check below (T651).
    // Idempotent — after the first boot the local dirs are empty.
    crate::storage::migrate_agents_to_systems();
    crate::storage::migrate_local_to_shared();

    let systems = crate::storage::load_prompts_for(PromptType::System);
    let default_id = library::default_system_id();

    let ps_mut = PromptState::get_mut(state);
    if let Some(active_id) = ps_mut.active_system_id.as_ref() {
        if !systems.iter().any(|a| a.id == *active_id) {
            ps_mut.active_system_id = Some(default_id.to_owned());
        }
    } else {
        ps_mut.active_system_id = Some(default_id.to_owned());
    }
}

/// Get the active system's content (system prompt).
/// Dynamically loads from disk every call — no caching.
#[must_use]
pub fn get_active_system_content(state: &State) -> String {
    let ps = PromptState::get(state);
    let systems = crate::storage::load_prompts_for(PromptType::System);
    if let Some(active_id) = ps.active_system_id.as_ref()
        && let Some(system) = systems.iter().find(|a| &a.id == active_id)
    {
        return system.content.clone();
    }
    // Fallback to default
    library::systems().first().map(|a| a.content.clone()).unwrap_or_default()
}
