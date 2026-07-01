use crate::modules;
use crate::state::{Kind, State};

// Re-export agent/seed functions from prompt module
pub(crate) use cp_mod_prompt::seed::{ensure_default_agent, get_active_agent_content};

/// Assign a UID to a panel if it doesn't have one.
fn assign_panel_uid(state: &mut State, context_type: &str) {
    if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type.as_str() == context_type)
        && ctx.uid.is_none()
    {
        ctx.uid = Some(format!("UID_{}_P", state.global_next_uid));
        state.global_next_uid = state.global_next_uid.saturating_add(1);
    }
}

/// Ensure all default context elements exist with correct IDs.
///
/// Uses the module registry to determine which fixed panels to create.
/// Conversation is special: it's always created but not numbered (no Px ID in sidebar).
/// P1 = Todo, P2 = Library, P3 = Overview, P4 = Tree, P5 = Memory,
/// P6 = Spine, P7 = Logs, P8 = Git, P9 = Scratchpad
pub(crate) fn ensure_default_contexts(state: &mut State) {
    // Ensure Conversation exists (special: no numbered Px, always first in context list)
    if !state.context.iter().any(|c| c.context_type.as_str() == Kind::CONVERSATION) {
        let elem = modules::make_default_entry("chat", Kind::new(Kind::CONVERSATION), "Chat", true);
        state.context.insert(0, elem);
    }

    let defaults = modules::all_fixed_panel_defaults();

    for (pos, d) in defaults.iter().enumerate() {
        // Core modules always get their panels; non-core only if active
        if !d.is_core && !state.active_modules.contains(d.module_id) {
            continue;
        }

        // Skip if panel already exists
        if state.context.iter().any(|c| c.context_type == d.context_type) {
            continue;
        }

        // pos is 0-indexed in FIXED_PANEL_ORDER, but IDs start at P1
        let id = format!("P{}", pos.saturating_add(1));

        // Evict any dynamic panel squatting on this fixed panel's ID.
        // This happens when a module is activated after dynamic panels already
        // claimed the slot (e.g., module was inactive at boot → slot looked free
        // → `next_available_context_id` assigned it to a dynamic panel → module
        // activated later → collision). Two panels sharing one ID breaks the
        // freeze system, cost tracking, and cache prefix matching.
        let squatter_new_id = state
            .context
            .iter()
            .any(|c| c.id == id && c.context_type != d.context_type)
            .then(|| state.next_available_context_id());
        if let Some(new_id) = squatter_new_id
            && let Some(squatter) = state.context.iter_mut().find(|c| c.id == id && c.context_type != d.context_type)
        {
            log::warn!(
                "Evicting panel '{}' (type={}) from ID {} → {new_id} to make room for fixed panel '{}'",
                squatter.name,
                squatter.context_type,
                id,
                d.display_name,
            );
            squatter.id = new_id;
        }

        let insert_pos = pos.saturating_add(1).min(state.context.len()); // +1 for Conversation at index 0
        let elem = modules::make_default_entry(&id, d.context_type.clone(), d.display_name, d.cache_deprecated);
        state.context.insert(insert_pos, elem);
    }

    // Assign UID to Conversation (needed for panels/ storage — it holds message_uids)
    assign_panel_uid(state, Kind::CONVERSATION);

    // Assign UIDs to all existing fixed panels (needed for panels/ storage)
    // Library panels don't need UIDs (rendered from in-memory state)
    for d in &defaults {
        if d.context_type.as_str() != Kind::LIBRARY && state.context.iter().any(|c| c.context_type == d.context_type) {
            assign_panel_uid(state, d.context_type.as_str());
        }
    }
}
