use crate::state::{ContextElement, ContextType, State};
use crate::tool_defs::estimate_tools_tokens;
use crate::tools::generate_directory_tree;

/// Ensure all default context elements exist with correct IDs
/// P1 = Main (Conversation), P2 = Directory (Tree), P3 = Todo, P4 = Memory, P5 = Overview, P6 = Tools
pub fn ensure_default_contexts(state: &mut State) {
    // Ensure Main conversation exists
    if !state.context.iter().any(|c| c.context_type == ContextType::Conversation && c.name == "Main") {
        state.context.insert(0, ContextElement {
            id: "P1".to_string(),
            context_type: ContextType::Conversation,
            name: "Main".to_string(),
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
        });
    }

    // Ensure Tree context element exists
    if !state.context.iter().any(|c| c.context_type == ContextType::Tree) {
        state.context.insert(1.min(state.context.len()), ContextElement {
            id: "P2".to_string(),
            context_type: ContextType::Tree,
            name: "Directory".to_string(),
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
        });
    }

    // Ensure Todo context element exists
    if !state.context.iter().any(|c| c.context_type == ContextType::Todo) {
        state.context.insert(2.min(state.context.len()), ContextElement {
            id: "P3".to_string(),
            context_type: ContextType::Todo,
            name: "Todo".to_string(),
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
        });
    }

    // Ensure Memory context element exists
    if !state.context.iter().any(|c| c.context_type == ContextType::Memory) {
        state.context.insert(3.min(state.context.len()), ContextElement {
            id: "P4".to_string(),
            context_type: ContextType::Memory,
            name: "Memory".to_string(),
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
        });
    }

    // Ensure Overview context element exists
    if !state.context.iter().any(|c| c.context_type == ContextType::Overview) {
        state.context.insert(4.min(state.context.len()), ContextElement {
            id: "P5".to_string(),
            context_type: ContextType::Overview,
            name: "Overview".to_string(),
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
        });
    }

    // Ensure Tools context element exists
    if !state.context.iter().any(|c| c.context_type == ContextType::Tools) {
        state.context.insert(5.min(state.context.len()), ContextElement {
            id: "P6".to_string(),
            context_type: ContextType::Tools,
            name: "Tools".to_string(),
            token_count: estimate_tools_tokens(&state.tools),
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
        });
    }

    // Generate initial tree to populate token count
    let _ = generate_directory_tree(state);
}
