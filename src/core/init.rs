use crate::modules;
use crate::state::{ContextType, State};

// Re-export agent/seed functions from prompt module
pub use cp_mod_prompt::seed::{ensure_default_agent, get_active_agent_content};

/// Assign a UID to a panel if it doesn't have one
fn assign_panel_uid(state: &mut State, context_type: ContextType) {
    if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == context_type)
        && ctx.uid.is_none()
    {
        ctx.uid = Some(format!("UID_{}_P", state.global_next_uid));
        state.global_next_uid += 1;
    }
}

/// Ensure all default context elements exist with correct IDs.
/// Uses the module registry to determine which fixed panels to create.
/// Conversation is special: it's always created but not numbered (no Px ID in sidebar).
/// P1 = Todo, P2 = Library, P3 = Overview, P4 = Tree, P5 = Memory,
/// P6 = Spine, P7 = Logs, P8 = Git, P9 = Scratchpad
pub fn ensure_default_contexts(state: &mut State) {
    // Ensure Conversation exists (special: no numbered Px, always first in context list)
    if !state.context.iter().any(|c| c.context_type == ContextType::CONVERSATION) {
        let elem = modules::make_default_context_element("chat", ContextType::new(ContextType::CONVERSATION), "Chat", true);
        state.context.insert(0, elem);
    }

    let defaults = modules::all_fixed_panel_defaults();

    for (pos, (module_id, is_core, ct, name, cache_deprecated)) in defaults.iter().enumerate() {
        // Core modules always get their panels; non-core only if active
        if !is_core && !state.active_modules.contains(*module_id) {
            continue;
        }

        // Skip if panel already exists
        if state.context.iter().any(|c| c.context_type == *ct) {
            continue;
        }

        // pos is 0-indexed in FIXED_PANEL_ORDER, but IDs start at P1
        let id = format!("P{}", pos + 1);
        let insert_pos = (pos + 1).min(state.context.len()); // +1 to account for Conversation at index 0
        let elem = modules::make_default_context_element(&id, *ct, name, *cache_deprecated);
        state.context.insert(insert_pos, elem);
    }

    // Assign UID to Conversation (needed for panels/ storage — it holds message_uids)
    assign_panel_uid(state, ContextType::new(ContextType::CONVERSATION));

    // Assign UIDs to all existing fixed panels (needed for panels/ storage)
    // Library panels don't need UIDs (rendered from in-memory state)
    for (_, _, ct, _, _) in &defaults {
        if *ct != ContextType::LIBRARY && state.context.iter().any(|c| c.context_type == *ct) {
            assign_panel_uid(state, *ct);
        }
    }
}
