use crate::state::{Message, State};
use crate::tool_defs::ToolDefinition;
use crate::tools::{
    generate_directory_tree, get_context_files, get_glob_context, get_grep_context,
    get_memory_context, get_overview_context, get_tmux_context, get_todo_context,
    refresh_conversation_context, refresh_file_hashes, refresh_glob_results, refresh_grep_results,
    refresh_memory_context, refresh_overview_context, refresh_tmux_context, refresh_todo_context,
    refresh_tools_context,
};

/// Context data prepared for streaming
pub struct StreamContext {
    pub messages: Vec<Message>,
    pub file_context: Vec<(String, String)>,
    pub glob_context: Vec<(String, String)>,
    pub grep_context: Vec<(String, String)>,
    pub tmux_context: Vec<(String, String)>,
    pub todo_context: String,
    pub memory_context: String,
    pub overview_context: String,
    pub directory_tree: String,
    pub tools: Vec<ToolDefinition>,
}

/// Refresh all context elements and prepare data for streaming
pub fn prepare_stream_context(state: &mut State, include_last_message: bool) -> StreamContext {
    // Refresh file hashes and token counts
    refresh_file_hashes(state);

    // Refresh all context element token counts
    refresh_conversation_context(state);
    refresh_glob_results(state);
    refresh_grep_results(state);
    refresh_tmux_context(state);
    refresh_todo_context(state);
    refresh_memory_context(state);
    refresh_overview_context(state);
    refresh_tools_context(state);

    // Get context content
    let file_context = get_context_files(state);
    let glob_context = get_glob_context(state);
    let grep_context = get_grep_context(state);
    let tmux_context = get_tmux_context(state);
    let todo_context = get_todo_context(state);
    let memory_context = get_memory_context(state);
    let overview_context = get_overview_context(state);
    let directory_tree = generate_directory_tree(state);

    // Prepare messages
    let messages: Vec<_> = if include_last_message {
        state.messages.iter()
            .filter(|m| !m.content.is_empty() || !m.tool_uses.is_empty() || !m.tool_results.is_empty())
            .cloned()
            .collect()
    } else {
        state.messages.iter()
            .filter(|m| !m.content.is_empty() || !m.tool_uses.is_empty() || !m.tool_results.is_empty())
            .take(state.messages.len().saturating_sub(1))
            .cloned()
            .collect()
    };

    StreamContext {
        messages,
        file_context,
        glob_context,
        grep_context,
        tmux_context,
        todo_context,
        memory_context,
        overview_context,
        directory_tree,
        tools: state.tools.clone(),
    }
}
