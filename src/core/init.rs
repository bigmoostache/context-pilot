use crate::constants::prompts;
use crate::state::{ContextElement, ContextType, State, SystemItem};

/// Ensure the default seed exists and there's always an active seed
pub fn ensure_default_seed(state: &mut State) {
    // Check if default seed (S0) exists
    let has_default = state.systems.iter().any(|s| s.id == prompts::DEFAULT_SEED_ID);

    if !has_default {
        // Create the default seed
        state.systems.insert(0, SystemItem {
            id: prompts::DEFAULT_SEED_ID.to_string(),
            name: prompts::DEFAULT_SEED_NAME.to_string(),
            description: prompts::DEFAULT_SEED_DESC.to_string(),
            content: prompts::DEFAULT_SEED_CONTENT.to_string(),
        });
    }

    // Ensure there's always an active seed
    if state.active_system_id.is_none() {
        state.active_system_id = Some(prompts::DEFAULT_SEED_ID.to_string());
    } else {
        // Verify the active seed still exists
        let active_id = state.active_system_id.as_ref().unwrap();
        if !state.systems.iter().any(|s| &s.id == active_id) {
            // Active seed was deleted, fall back to default
            state.active_system_id = Some(prompts::DEFAULT_SEED_ID.to_string());
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
    prompts::DEFAULT_SEED_CONTENT.to_string()
}

/// Ensure all default context elements exist with correct IDs
/// P0 = Seed (System), P1 = Chat (Conversation), P2 = Tree, P3 = WIP (Todo),
/// P4 = Notes (Memory), P5 = World (Overview), P6 = Changes (Git)
pub fn ensure_default_contexts(state: &mut State) {
    // Ensure System context element exists (P0)
    if !state.context.iter().any(|c| c.context_type == ContextType::System) {
        state.context.insert(0, ContextElement {
            id: "P0".to_string(),
            context_type: ContextType::System,
            name: "Seed".to_string(),
            token_count: 0,
            file_path: None,
            file_hash: None,
            glob_pattern: None,
            glob_path: None,
            grep_pattern: None,
            grep_path: None,
            grep_file_pattern: None,
            tmux_pane_id: None,
            tmux_lines: None,
            tmux_last_keys: None,
            tmux_description: None,
            cached_content: None,
            cache_deprecated: false,
            last_refresh_ms: 0,
            tmux_last_lines_hash: None,
        });
    }

    // Ensure Conversation exists (P1)
    if !state.context.iter().any(|c| c.context_type == ContextType::Conversation) {
        state.context.insert(1.min(state.context.len()), ContextElement {
            id: "P1".to_string(),
            context_type: ContextType::Conversation,
            name: "Chat".to_string(),
            token_count: 0,
            file_path: None,
            file_hash: None,
            glob_pattern: None,
            glob_path: None,
            grep_pattern: None,
            grep_path: None,
            grep_file_pattern: None,
            tmux_pane_id: None,
            tmux_lines: None,
            tmux_last_keys: None,
            tmux_description: None,
            cached_content: None,
            cache_deprecated: true,
            last_refresh_ms: 0,
            tmux_last_lines_hash: None,
        });
    }

    // Ensure Tree context element exists (P2)
    if !state.context.iter().any(|c| c.context_type == ContextType::Tree) {
        state.context.insert(2.min(state.context.len()), ContextElement {
            id: "P2".to_string(),
            context_type: ContextType::Tree,
            name: "Tree".to_string(),
            token_count: 0,
            file_path: None,
            file_hash: None,
            glob_pattern: None,
            glob_path: None,
            grep_pattern: None,
            grep_path: None,
            grep_file_pattern: None,
            tmux_pane_id: None,
            tmux_lines: None,
            tmux_last_keys: None,
            tmux_description: None,
            cached_content: None,
            cache_deprecated: true,
            last_refresh_ms: 0,
            tmux_last_lines_hash: None,
        });
    }

    // Ensure Todo context element exists (P3)
    if !state.context.iter().any(|c| c.context_type == ContextType::Todo) {
        state.context.insert(3.min(state.context.len()), ContextElement {
            id: "P3".to_string(),
            context_type: ContextType::Todo,
            name: "WIP".to_string(),
            token_count: 0,
            file_path: None,
            file_hash: None,
            glob_pattern: None,
            glob_path: None,
            grep_pattern: None,
            grep_path: None,
            grep_file_pattern: None,
            tmux_pane_id: None,
            tmux_lines: None,
            tmux_last_keys: None,
            tmux_description: None,
            cached_content: None,
            cache_deprecated: false,
            last_refresh_ms: 0,
            tmux_last_lines_hash: None,
        });
    }

    // Ensure Memory context element exists (P4)
    if !state.context.iter().any(|c| c.context_type == ContextType::Memory) {
        state.context.insert(4.min(state.context.len()), ContextElement {
            id: "P4".to_string(),
            context_type: ContextType::Memory,
            name: "Notes".to_string(),
            token_count: 0,
            file_path: None,
            file_hash: None,
            glob_pattern: None,
            glob_path: None,
            grep_pattern: None,
            grep_path: None,
            grep_file_pattern: None,
            tmux_pane_id: None,
            tmux_lines: None,
            tmux_last_keys: None,
            tmux_description: None,
            cached_content: None,
            cache_deprecated: false,
            last_refresh_ms: 0,
            tmux_last_lines_hash: None,
        });
    }

    // Ensure Overview context element exists (P5)
    if !state.context.iter().any(|c| c.context_type == ContextType::Overview) {
        state.context.insert(5.min(state.context.len()), ContextElement {
            id: "P5".to_string(),
            context_type: ContextType::Overview,
            name: "World".to_string(),
            token_count: 0,
            file_path: None,
            file_hash: None,
            glob_pattern: None,
            glob_path: None,
            grep_pattern: None,
            grep_path: None,
            grep_file_pattern: None,
            tmux_pane_id: None,
            tmux_lines: None,
            tmux_last_keys: None,
            tmux_description: None,
            cached_content: None,
            cache_deprecated: false,
            last_refresh_ms: 0,
            tmux_last_lines_hash: None,
        });
    }

    // Ensure Git context element exists (P6)
    if !state.context.iter().any(|c| c.context_type == ContextType::Git) {
        state.context.insert(6.min(state.context.len()), ContextElement {
            id: "P6".to_string(),
            context_type: ContextType::Git,
            name: "Changes".to_string(),
            token_count: 0,
            file_path: None,
            file_hash: None,
            glob_pattern: None,
            glob_path: None,
            grep_pattern: None,
            grep_path: None,
            grep_file_pattern: None,
            tmux_pane_id: None,
            tmux_lines: None,
            tmux_last_keys: None,
            tmux_description: None,
            cached_content: None,
            cache_deprecated: false,
            last_refresh_ms: 0,
            tmux_last_lines_hash: None,
        });
    }
}
