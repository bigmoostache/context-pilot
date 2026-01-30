use crate::constants::{MAX_CONTEXT_TOKENS, CLEANING_THRESHOLD, prompts, icons};
use crate::state::State;
use crate::tool_defs::{get_all_tool_definitions, ToolDefinition};

/// Tool IDs that are allowed for context cleaning
const CLEANER_TOOL_IDS: &[&str] = &[
    "close_contexts",
    "set_message_status",
    "update_todos",
    "update_memories",
];

/// Get the context-cleaning specific tools (filtered from existing tools)
pub fn get_cleaner_tools() -> Vec<ToolDefinition> {
    get_all_tool_definitions()
        .into_iter()
        .filter(|t| CLEANER_TOOL_IDS.contains(&t.id.as_str()))
        .collect()
}


/// Calculate current context usage
pub fn calculate_context_usage(state: &State) -> (usize, f32) {
    let total: usize = state.context.iter().map(|c| c.token_count).sum();
    let percentage = total as f32 / MAX_CONTEXT_TOKENS as f32;
    (total, percentage)
}

/// Check if context cleaning should be triggered
pub fn should_clean_context(state: &State) -> bool {
    if state.is_cleaning_context {
        return false;
    }
    let (_, percentage) = calculate_context_usage(state);
    percentage >= CLEANING_THRESHOLD
}

/// Build the context overview for the cleaner
pub fn build_cleaner_context(state: &State) -> String {
    let mut context = String::new();

    context.push_str("=== CONTEXT OVERVIEW ===\n\n");

    // Context elements with sizes
    context.push_str("## Context Elements:\n");
    for ctx in &state.context {
        let size_indicator = if ctx.token_count > 10000 {
            format!("{} LARGE", icons::SIZE_LARGE)
        } else if ctx.token_count > 5000 {
            format!("{} MEDIUM", icons::SIZE_MEDIUM)
        } else {
            format!("{} SMALL", icons::SIZE_SMALL)
        };

        let details = match ctx.context_type {
            crate::state::ContextType::File => {
                format!(" - {}", ctx.file_path.as_deref().unwrap_or("unknown"))
            }
            crate::state::ContextType::Glob => {
                format!(" - pattern: {}", ctx.glob_pattern.as_deref().unwrap_or("?"))
            }
            crate::state::ContextType::Tmux => {
                format!(" - {}", ctx.tmux_description.as_deref().unwrap_or("terminal"))
            }
            _ => String::new(),
        };

        context.push_str(&format!(
            "{} {} [{}] {} ({} tokens){}\n",
            size_indicator,
            ctx.id,
            format!("{:?}", ctx.context_type).to_lowercase(),
            ctx.name,
            ctx.token_count,
            details
        ));
    }

    // Messages summary
    context.push_str("\n## Messages:\n");
    for msg in &state.messages {
        let status_str = match msg.status {
            crate::state::MessageStatus::Full => "full",
            crate::state::MessageStatus::Summarized => "summarized",
            crate::state::MessageStatus::Deleted => "deleted",
        };

        let type_str = match msg.message_type {
            crate::state::MessageType::TextMessage => "text",
            crate::state::MessageType::ToolCall => "tool_call",
            crate::state::MessageType::ToolResult => "tool_result",
        };

        // Truncate content preview
        let preview: String = msg.content.chars().take(80).collect();
        let preview = preview.replace('\n', " ");
        let ellipsis = if msg.content.len() > 80 { "..." } else { "" };

        context.push_str(&format!(
            "{} [{}] {} ({} tokens, {}) - \"{}{}\"\n",
            msg.id,
            msg.role,
            type_str,
            msg.content_token_count,
            status_str,
            preview,
            ellipsis
        ));
    }

    // Todos
    if !state.todos.is_empty() {
        context.push_str("\n## Todos:\n");
        for todo in &state.todos {
            context.push_str(&format!(
                "{} [{}] {}\n",
                todo.id,
                todo.status.icon(),
                todo.name
            ));
        }
    }

    // Memories
    if !state.memories.is_empty() {
        context.push_str("\n## Memories:\n");
        for mem in &state.memories {
            let preview: String = mem.content.chars().take(60).collect();
            context.push_str(&format!(
                "{} [{}] \"{}\"\n",
                mem.id,
                mem.importance.as_str(),
                preview
            ));
        }
    }

    // Usage summary
    let (total, percentage) = calculate_context_usage(state);
    context.push_str(&format!(
        "\n## Usage: {} / {} tokens ({:.1}%)\n",
        total,
        MAX_CONTEXT_TOKENS,
        percentage * 100.0
    ));
    context.push_str(&format!("## Target: Reduce to below {:.0}%\n", CLEANING_THRESHOLD * 100.0 - 20.0));

    context
}

/// Get the system prompt for the cleaner
pub fn get_cleaner_system_prompt() -> &'static str {
    prompts::CLEANER_SYSTEM
}

